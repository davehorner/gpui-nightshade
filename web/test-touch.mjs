import assert from 'node:assert/strict';
import {installViewportTouch} from '../crates/gpui-nightshade-components/src/touch.js';

class Input extends Event {
  constructor(type, init) {
    super(type, init);
    const {bubbles, cancelable, ...fields} = init;
    Object.assign(this, fields);
  }
}
globalThis.PointerEvent = Input;
globalThis.WheelEvent = Input;
const surface = new EventTarget(), canvas = new EventTarget(), events = [];
canvas.getBoundingClientRect = () => ({left:10, top:20});
for (const type of ['pointerdown','pointermove','pointerup','pointercancel','wheel'])
  canvas.addEventListener(type, e => events.push(e));
const navigation = [];
const dispose = installViewportTouch(canvas, () => [0,0,500,500], surface, (kind,x,y) => navigation.push({kind,x,y}));
const touch = (type, id, x, y) => {
  const event = new Input(type, {pointerType:'touch', pointerId:id, clientX:x, clientY:y, cancelable:true});
  surface.dispatchEvent(event);
  return event;
};
assert.equal(touch('pointerdown',1,100,100).defaultPrevented, false);
assert.equal(touch('pointermove',1,110,100).defaultPrevented, false);
assert.equal(touch('pointerdown',2,210,100).defaultPrevented, true);
assert.equal(events.filter(e=>e.type === 'pointercancel').length, 2);
touch('pointermove',2,230,120);
assert.equal(navigation[0].kind, 'pan');
assert(navigation.some(e => e.kind === 'orbit' && e.x < 0), 'two-finger twist rotates');
assert(navigation.at(-1).x > 0, 'spreading fingers zooms in');
assert.equal(navigation.at(-1).kind, 'zoom');
touch('pointerdown',3,300,200);
const beforeOrbit = navigation.length;
touch('pointermove',3,310,210);
assert.equal(navigation.length, beforeOrbit + 1);
assert.equal(navigation.at(-1).kind, 'orbit');
touch('pointerup',3,310,210);
const beforeRebase = navigation.length;
touch('pointermove',2,230,120);
assert.equal(navigation.at(-2).kind, 'pan');
assert.equal(navigation.at(-2).x, 0, 'finger removal resets baseline');
touch('pointerup',2,230,120);
const beforeSingle = navigation.length;
assert.equal(touch('pointermove',1,120,100).defaultPrevented, true);
assert.equal(navigation.length, beforeSingle, 'remaining finger cannot move the camera');
touch('pointerup',1,120,100);
assert.equal(touch('pointerdown',4,120,100).defaultPrevented, false, 'next single-finger gesture works');
touch('pointerdown',5,220,100);
surface.dispatchEvent(new Event('blur'));
const afterBlur = navigation.length;
touch('pointermove',4,130,100);
assert.equal(navigation.length, afterBlur);
assert.equal(touch('pointerdown',6,900,900).defaultPrevented, false, 'outside viewport untouched');
dispose();
navigation.length = 0;
const disposeTwist = installViewportTouch(canvas, () => [0,0,500,500], surface,
  (kind,x,y) => navigation.push({kind,x,y}));
touch('pointerdown',10,300,250);
touch('pointerdown',11,200,250);
touch('pointermove',11,200,249);
const seam = navigation.find(e => e.kind === 'orbit');
assert(seam && Math.abs(seam.x) < 2, 'twist across +/- pi stays small');
touch('pointerup',11,200,249);
touch('pointerdown',12,250,300);
const beforeReset = navigation.length;
touch('pointermove',12,250,300);
assert(!navigation.slice(beforeReset).some(e => e.kind === 'orbit'), 'new finger resets twist baseline');
disposeTwist();
console.log('Multi-touch navigation tests passed');
