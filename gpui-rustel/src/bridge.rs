//! Native client of the existing music hub. Audio remains in Rustel/Bespoke.
use music_link::{AudioFrame, Command, Response, Snapshot};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct Received<T> {
    pub value: Arc<T>,
    pub at: Instant,
}
#[derive(Default)]
pub struct State {
    pub sessions: BTreeMap<String, Received<Snapshot>>,
    pub audio: BTreeMap<String, Received<AudioFrame>>,
    pub status: String,
    pub feedback: String,
    pub pending: BTreeMap<String, Instant>,
}
impl State {
    pub fn prune(&mut self) {
        self.sessions
            .retain(|_, v| v.at.elapsed() < Duration::from_secs(3));
        self.audio.retain(|session, v| {
            v.at.elapsed() < Duration::from_secs(1)
                && self
                    .sessions
                    .get(session)
                    .is_some_and(|s| s.value.generation == v.value.generation)
        });
        let before = self.pending.len();
        self.pending
            .retain(|_, at| at.elapsed() < Duration::from_secs(5));
        if before != self.pending.len() {
            self.feedback = "Command acknowledgement timed out".into();
        }
    }
    pub fn receive_snapshot(&mut self, value: Snapshot) {
        if !valid_snapshot(&value)
            || (!self.sessions.contains_key(&value.session) && self.sessions.len() >= 32)
        {
            return;
        }
        self.sessions.insert(
            value.session.clone(),
            Received {
                value: Arc::new(value),
                at: Instant::now(),
            },
        );
    }
    pub fn receive_audio(&mut self, value: AudioFrame) {
        if !value.valid()
            || self
                .sessions
                .get(&value.session)
                .is_none_or(|s| s.value.generation != value.generation)
        {
            return;
        }
        if self.audio.get(&value.session).is_some_and(|old| {
            old.value.generation == value.generation
                && old.value.stream_id == value.stream_id
                && old.value.epoch == value.epoch
                && old.value.sequence >= value.sequence
        }) {
            return;
        }
        self.audio.insert(
            value.session.clone(),
            Received {
                value: Arc::new(value),
                at: Instant::now(),
            },
        );
    }
}
pub fn valid_snapshot(s: &Snapshot) -> bool {
    s.version == 1
        && s.session.len() <= 128
        && !s.session.is_empty()
        && s.source.len() <= 512 * 1024
        && s.events.len() <= 2048
        && s.lanes.len() <= 64
        && s.cycle.is_finite()
        && s.cps.is_finite()
        && s.cps > 0.
        && s.cps <= 100.
        && s.events.iter().all(|e| {
            e.cycle.is_finite()
                && e.duration.is_finite()
                && e.duration >= 0.
                && e.pitch.is_none_or(f64::is_finite)
                && e.gain.is_finite()
        })
}
pub type Shared = Arc<Mutex<State>>;
pub struct Bridge {
    pub shared: Shared,
    commands: mpsc::SyncSender<Command>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    serial: u64,
}
impl Bridge {
    pub fn connect(address: String) -> std::io::Result<Self> {
        let shared = Arc::new(Mutex::new(State {
            status: format!("Connecting to {address}"),
            ..Default::default()
        }));
        let state = shared.clone();
        let (commands, receiver) = mpsc::sync_channel::<Command>(64);
        let (stop, mut stopped) = tokio::sync::oneshot::channel();
        std::thread::Builder::new().name("gpui-rustel-hub".into()).spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            runtime.block_on(async move {
                loop {
                    let client = hearsay::Client::new("gpui-rustel", hearsay::ClientSettings {
                        max_connection_attempts: Some(1), autoreconnect: false, ..Default::default()
                    });
                    let connected = tokio::select! {
                        _ = &mut stopped => return,
                        result = client.connect(&address) => result,
                    };
                    let subscribed = if connected.is_ok() {
                        client.subscribe(&[music_link::SNAPSHOT_TOPIC, music_link::AUDIO_TOPIC, music_link::RESPONSE_TOPIC]).await
                    } else { connected };
                    if let Err(error) = subscribed {
                        state.lock().unwrap().status = format!("Reconnecting to {address}: {error}");
                        tokio::select! {_ = &mut stopped => return, _ = tokio::time::sleep(Duration::from_secs(1)) => {}}
                        continue;
                    }
                    state.lock().unwrap().status = "Connected  -  waiting for Rustel".into();
                    let mut timer = tokio::time::interval(Duration::from_millis(20));
                    loop { tokio::select! {
                        _ = &mut stopped => return,
                        _ = timer.tick() => {
                            state.lock().unwrap().prune();
                            for command in receiver.try_iter().take(64) {
                                let fresh = state.lock().unwrap().sessions.get(&command.session)
                                    .is_some_and(|s| s.at.elapsed() < Duration::from_secs(3) && s.value.generation == command.generation);
                                if !fresh {state.lock().unwrap().pending.remove(&command.request); continue;}
                                let result = tokio::select! {
                                    _ = &mut stopped => return,
                                    result = tokio::time::timeout(Duration::from_secs(2),client.publish_json(music_link::COMMAND_TOPIC, &command, hearsay::Route::Global)) => result,
                                };
                                if !matches!(result, Ok(Ok(()))) {
                                    let mut state = state.lock().unwrap();
                                    state.pending.remove(&command.request); state.feedback = "Command could not reach hub".into();
                                }
                            }
                        },
                        message = client.next_message() => {
                            let Some(message) = message else {break};
                            let mut state = state.lock().unwrap();
                            if message.topic() == music_link::SNAPSHOT_TOPIC {
                                if let Ok(snapshot) = message.read_json::<Snapshot>() { state.receive_snapshot(snapshot); }
                            } else if message.topic() == music_link::AUDIO_TOPIC {
                                if let Ok(audio) = message.read_json::<AudioFrame>() { state.receive_audio(audio); }
                            } else if let Ok(response) = message.read_json::<Response>()
                                && state.pending.remove(&response.request).is_some() {
                                    state.feedback = if response.accepted { "Applied".into() } else { response.message };
                            }
                        }
                    }}
                    {
                        let mut state=state.lock().unwrap();state.sessions.clear();state.audio.clear();state.pending.clear();
                        state.status="Hub disconnected  -  reconnecting".into();
                    }
                    while receiver.try_recv().is_ok() {}
                    tokio::select! {_ = &mut stopped => return, _ = tokio::time::sleep(Duration::from_secs(1)) => {}}
                }
            });
        })?;
        Ok(Self {
            shared,
            commands,
            stop: Some(stop),
            serial: 0,
        })
    }
    pub fn send(&mut self, snapshot: &Snapshot, mut command: Command) {
        self.serial += 1;
        command.request = format!("gpui-{}-{}", std::process::id(), self.serial);
        command.session = snapshot.session.clone();
        command.generation = snapshot.generation;
        let mut state = self.shared.lock().unwrap();
        let error = if state.sessions.get(&snapshot.session).is_none_or(|s| {
            s.value.generation != snapshot.generation || s.at.elapsed() >= Duration::from_secs(3)
        }) {
            Some("Session is stale".to_owned())
        } else {
            command.validate(snapshot).err().map(str::to_owned)
        };
        if let Some(error) = error {
            state.feedback = error;
            return;
        }
        if state.pending.len() >= 64 {
            state.feedback = "Too many pending commands".into();
            return;
        }
        let request = command.request.clone();
        if self.commands.try_send(command).is_ok() {
            state.pending.insert(request, Instant::now());
            state.feedback = "Applying…".into();
        } else {
            state.feedback = "Command queue unavailable".into();
        }
    }
}
impl Drop for Bridge {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}
pub fn command() -> Command {
    Command {
        demo: None,
        tempo: None,
        source_update: None,
        transport: None,
        request: String::new(),
        session: String::new(),
        generation: 0,
        lane: String::new(),
        semitones: 0,
        solo: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> Snapshot {
        Snapshot {
            version: 1,
            session: "native-test".into(),
            generation: 4,
            cps: 0.5,
            transport: Some(music_link::TransportState {
                paused: false,
                view_cycle: None,
            }),
            lanes: vec![music_link::Lane {
                id: "#0".into(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }
    #[test]
    fn rejects_invalid_snapshots_and_expires_replaced_audio() {
        let mut state = State::default();
        let mut s = snapshot();
        s.cps = f64::NAN;
        state.receive_snapshot(s);
        assert!(state.sessions.is_empty());
        state.receive_snapshot(snapshot());
        let audio = AudioFrame {
            version: 1,
            session: "native-test".into(),
            generation: 4,
            sample_rate: 48000,
            sequence: 2,
            scope: vec![0.; 512],
            spectrum: vec![-100.; 512],
            ..Default::default()
        };
        state.receive_audio(audio.clone());
        assert_eq!(state.audio.len(), 1);
        state.receive_audio(AudioFrame {
            sequence: 1,
            ..audio.clone()
        });
        assert_eq!(state.audio["native-test"].value.sequence, 2);
        let mut s = snapshot();
        s.generation = 5;
        state.receive_snapshot(s);
        state.prune();
        assert!(state.audio.is_empty());
        state.sessions.get_mut("native-test").unwrap().at = Instant::now() - Duration::from_secs(4);
        state.prune();
        assert!(state.sessions.is_empty());
        let mut invalid = audio;
        invalid.scope[0] = f32::INFINITY;
        state.receive_audio(invalid);
        assert!(state.audio.is_empty());
    }
    #[tokio::test(flavor = "current_thread")]
    async fn native_hub_snapshot_command_and_acknowledgement() {
        let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reservation.local_addr().unwrap().to_string();
        drop(reservation);
        let mut bridge = Bridge::connect(address.clone()).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let _broker = hearsay::Broker::start(&address).await.unwrap();
        let producer = hearsay::Client::new("native-test-producer", Default::default());
        producer.connect(&address).await.unwrap();
        producer
            .subscribe(&[music_link::COMMAND_TOPIC])
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                producer
                    .publish_json(
                        music_link::SNAPSHOT_TOPIC,
                        &snapshot(),
                        hearsay::Route::Global,
                    )
                    .await
                    .unwrap();
                tokio::time::sleep(Duration::from_millis(25)).await;
                if bridge
                    .shared
                    .lock()
                    .unwrap()
                    .sessions
                    .contains_key("native-test")
                {
                    break;
                }
            }
        })
        .await
        .unwrap();
        let mut c = command();
        c.transport = Some(music_link::TransportCommand::Pause);
        bridge.send(&snapshot(), c);
        let message = tokio::time::timeout(Duration::from_secs(3), producer.next_message())
            .await
            .unwrap()
            .unwrap();
        let c = message.read_json::<Command>().unwrap();
        assert_eq!(c.generation, 4);
        assert_eq!(c.session, "native-test");
        assert!(matches!(
            c.transport,
            Some(music_link::TransportCommand::Pause)
        ));
        producer
            .publish_json(
                music_link::RESPONSE_TOPIC,
                &Response {
                    request: c.request,
                    accepted: true,
                    message: String::new(),
                },
                hearsay::Route::Global,
            )
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !bridge.shared.lock().unwrap().pending.is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(bridge.shared.lock().unwrap().feedback, "Applied");
        let mut replaced = snapshot();
        replaced.generation = 3;
        bridge.send(&replaced, command());
        assert_eq!(bridge.shared.lock().unwrap().feedback, "Session is stale");
        _broker.stop();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !bridge.shared.lock().unwrap().sessions.is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let _replacement = hearsay::Broker::start(&address).await.unwrap();
        let producer = hearsay::Client::new("replacement-producer", Default::default());
        producer.connect(&address).await.unwrap();
        let mut replacement = snapshot();
        replacement.generation = 5;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                producer
                    .publish_json(
                        music_link::SNAPSHOT_TOPIC,
                        &replacement,
                        hearsay::Route::Global,
                    )
                    .await
                    .unwrap();
                tokio::time::sleep(Duration::from_millis(25)).await;
                if bridge
                    .shared
                    .lock()
                    .unwrap()
                    .sessions
                    .get("native-test")
                    .is_some_and(|s| s.value.generation == 5)
                {
                    break;
                }
            }
        })
        .await
        .unwrap();
    }
}
