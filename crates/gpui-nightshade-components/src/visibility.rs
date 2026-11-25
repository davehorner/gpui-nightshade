/// Independent visibility sources: restoring a window must not unhide a dock tab.
pub(crate) struct PresentationVisibility {
    panel: bool,
    window: bool,
}

impl Default for PresentationVisibility {
    fn default() -> Self {
        Self {
            panel: true,
            window: true,
        }
    }
}

impl PresentationVisibility {
    pub fn is_presented(&self) -> bool {
        self.panel && self.window
    }

    pub fn set_panel(&mut self, visible: bool) -> bool {
        let before = self.is_presented();
        self.panel = visible;
        before != self.is_presented()
    }

    pub fn set_window(&mut self, visible: bool) -> bool {
        let before = self.is_presented();
        self.window = visible;
        before != self.is_presented()
    }
}

#[cfg(test)]
mod tests {
    use super::PresentationVisibility;

    #[test]
    fn restoring_window_does_not_resume_a_hidden_panel() {
        let mut visibility = PresentationVisibility::default();
        assert!(visibility.set_window(false));
        assert!(!visibility.set_panel(false));
        assert!(!visibility.set_window(true));
        assert!(!visibility.is_presented());
        assert!(visibility.set_panel(true));
        assert!(visibility.is_presented());
        assert!(!visibility.set_window(true));
    }

    #[test]
    fn showing_panel_does_not_resume_a_hidden_window() {
        let mut visibility = PresentationVisibility::default();
        assert!(visibility.set_panel(false));
        assert!(!visibility.set_window(false));
        assert!(!visibility.set_panel(true));
        assert!(!visibility.is_presented());
        assert!(visibility.set_window(true));
        assert!(visibility.is_presented());
    }
}
