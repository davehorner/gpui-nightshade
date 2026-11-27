// Multi-touch navigation is scoped to the rendered viewport. Single touches
// remain with GPUI, including taps, controls, and its existing drag-to-zoom.
const diagnostics = {downs:0, moves:0, accepted:0, navigation:0, applied:0};
export function installViewportTouch(canvas, getBounds, surface = window, navigate = queueNavigation) {
  const fingers = new Map();
  let captured = false, baseline = null, forwarding = false;
  const inside = p => {
    const b = getBounds(), r = canvas.getBoundingClientRect();
    const x = p.clientX - r.left, y = p.clientY - r.top;
    return b && x >= b[0] && y >= b[1] && x < b[0] + b[2] && y < b[1] + b[3];
  };
  const sample = () => {
    const p = [...fingers.values()];
    return {x:p.reduce((s,v)=>s+v.clientX,0)/p.length,
      y:p.reduce((s,v)=>s+v.clientY,0)/p.length,
      distance:p.length === 2 ? Math.hypot(p[1].clientX-p[0].clientX,p[1].clientY-p[0].clientY) : 0,
      angle:p.length === 2 ? Math.atan2(p[1].clientY-p[0].clientY,p[1].clientX-p[0].clientX) : 0,
      count:p.length};
  };
  const emit = event => {
    forwarding = true;
    try { canvas.dispatchEvent(event); } finally { forwarding = false; }
  };
  const rebase = () => { baseline = fingers.size ? sample() : null; };
  const handle = event => {
    if (forwarding || event.pointerType !== 'touch') return;
    if (event.type === 'pointerdown') diagnostics.downs++;
    if (event.type === 'pointermove') diagnostics.moves++;
    if (event.type === 'pointerdown') {
      if (!inside(event)) return;
      diagnostics.accepted++;
      fingers.set(event.pointerId, event);
      if (fingers.size < 2 && !captured) return;
      if (!captured) {
        // Unwind GPUI's single-finger scroll/tap recognizer before taking over.
        for (const [id,p] of fingers) emit(new PointerEvent('pointercancel', {
          bubbles:true, pointerType:'touch', pointerId:id,
          clientX:p.clientX, clientY:p.clientY
        }));
        captured = true;
      }
      rebase();
    } else {
      if (!fingers.has(event.pointerId)) return;
      if (event.type === 'pointermove') {
        fingers.set(event.pointerId, event);
        if (!captured) return;
        if (fingers.size >= 2) {
          const next = sample();
          navigate(next.count === 2 ? 'pan' : 'orbit', next.x - baseline.x, next.y - baseline.y, next);
          if (next.count === 2 && baseline.distance > 1 && next.distance > 1) {
            // Wrap across +/- pi so crossing the seam cannot spin the camera.
            const angle = next.angle - baseline.angle;
            const twist = Math.atan2(Math.sin(angle), Math.cos(angle));
            if (Math.abs(twist) > 1e-6) navigate('orbit', -twist / 0.01, 0, next);
            navigate('zoom', Math.log(next.distance / baseline.distance), 0, next);
          }
          baseline = next;
        }
      } else {
        fingers.delete(event.pointerId);
        if (!captured) return;
        rebase();
      }
    }
    event.preventDefault();
    event.stopImmediatePropagation();
    if (!fingers.size) captured = false;
  };
  const cancel = () => { fingers.clear(); captured = false; baseline = null; pending.fill(0); };
  for (const type of ['pointerdown','pointermove','pointerup','pointercancel'])
    surface.addEventListener(type, handle, {capture:true, passive:false});
  surface.addEventListener('blur', cancel);
  surface.addEventListener('resize', cancel);
  return () => {
    cancel();
    for (const type of ['pointerdown','pointermove','pointerup','pointercancel'])
      surface.removeEventListener(type, handle, true);
    surface.removeEventListener('blur', cancel);
    surface.removeEventListener('resize', cancel);
  };
}

let bounds, dispose;
// Bounded, coalesced camera deltas; no synthetic mouse focus/IME events.
const pending = new Float32Array(8);
const empty = new Float32Array(0);
function queueNavigation(kind, x, y, center) {
  diagnostics.navigation++;
  const offset = kind === 'pan' ? 0 : kind === 'orbit' ? 2 : 4;
  pending[offset] += x;
  if (kind !== 'zoom') pending[offset + 1] += y;
  const r = document.querySelector('body > canvas')?.getBoundingClientRect();
  pending[5] = center.x - (r?.left || 0);
  pending[6] = center.y - (r?.top || 0);
  pending[7] = 1;
}
export function takeViewportTouch() {
  if (!pending[7]) return empty;
  diagnostics.applied++;
  const result = pending.slice();
  pending.fill(0);
  return result;
}
let pose;
export function viewportTouchState() { return {bounds, pose, diagnostics}; }
export function viewportTouchBounds(x,y,width,height,yaw,pitch,distance,tx,ty,tz) {
  bounds = [x,y,width,height];
  pose = {yaw,pitch,distance,target:[tx,ty,tz]};
  if (!dispose) {
    const canvas = document.querySelector('body > canvas');
    if (canvas) dispose = installViewportTouch(canvas, () => bounds);
  }
}
export function clearViewportTouch() {
  dispose?.(); dispose = null; bounds = null; pending.fill(0);
}
