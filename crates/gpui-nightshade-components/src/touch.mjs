// Multi-touch navigation is scoped to the rendered viewport. Single touches
// remain with GPUI, including taps, controls, and its existing drag-to-zoom.
export function installViewportTouch(canvas, getBounds, surface = window) {
  const fingers = new Map();
  let captured = false, baseline = null, dragging = false, forwarding = false;
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
      count:p.length};
  };
  const emit = event => {
    forwarding = true;
    try { canvas.dispatchEvent(event); } finally { forwarding = false; }
  };
  const mouse = (type, p, shiftKey=false) => emit(new PointerEvent(type, {
    bubbles:true, pointerId:2147483646, pointerType:'mouse',
    clientX:p.x, clientY:p.y, button:2, buttons:type === 'pointerup' ? 0 : 2, shiftKey
  }));
  const release = () => {
    if (dragging) mouse('pointerup', baseline);
    dragging = false;
  };
  const rebase = () => {
    release();
    baseline = fingers.size ? sample() : null;
    if (fingers.size >= 2) {
      mouse('pointerdown', baseline, fingers.size === 2);
      dragging = true;
    }
  };
  const handle = event => {
    if (forwarding || event.pointerType !== 'touch') return;
    if (event.type === 'pointerdown') {
      if (!inside(event)) return;
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
          mouse('pointermove', next, next.count === 2);
          if (next.count === 2 && baseline.distance > 1 && next.distance > 1) {
            emit(new WheelEvent('wheel', {bubbles:true, cancelable:true,
              clientX:next.x, clientY:next.y,
              deltaY:-Math.log(next.distance / baseline.distance) / 0.003}));
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
  const cancel = () => { release(); fingers.clear(); captured = false; baseline = null; };
  for (const type of ['pointerdown','pointermove','pointerup','pointercancel'])
    surface.addEventListener(type, handle, {capture:true, passive:false});
  surface.addEventListener('blur', cancel);
  return () => {
    cancel();
    for (const type of ['pointerdown','pointermove','pointerup','pointercancel'])
      surface.removeEventListener(type, handle, true);
    surface.removeEventListener('blur', cancel);
  };
}

let bounds, dispose;
export function viewportTouchBounds(x,y,width,height) {
  bounds = [x,y,width,height];
  if (!dispose) {
    const canvas = document.querySelector('body > canvas');
    if (canvas) dispose = installViewportTouch(canvas, () => bounds);
  }
}
export function clearViewportTouch() {
  dispose?.(); dispose = null; bounds = null;
}
