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
const dispose = installViewportTouch(canvas, () => [0,0,500,500], surface);
const touch = (type, id, x, y) => {
  const event = new Input(type, {pointerType:'touch', pointerId:id, clientX:x, clientY:y, cancelable:true});
  surface.dispatchEvent(event);
  return event;
};
assert.equal(touch('pointerdown',1,100,100).defaultPrevented, false);
assert.equal(touch('pointermove',1,110,100).defaultPrevented, false);
assert.equal(touch('pointerdown',2,210,100).defaultPrevented, true);
assert.equal(events.filter(e=>e.type === 'pointercancel').length, 2);
assert.equal(events.at(-1).shiftKey, true);
touch('pointermove',2,230,120);
assert.equal(events.at(-2).type, 'pointermove');
assert.equal(events.at(-2).shiftKey, true);
assert(events.at(-1).deltaY < 0, 'spreading fingers zooms in');
touch('pointerdown',3,300,200);
assert.equal(events.at(-1).type, 'pointerdown');
assert.equal(events.at(-1).shiftKey, false, 'three fingers orbit');
touch('pointermove',3,310,210);
assert.equal(events.at(-1).type, 'pointermove');
assert.equal(events.at(-1).shiftKey, false);
touch('pointerup',3,310,210);
assert.equal(events.at(-1).shiftKey, true, 'returning to two fingers resets pan baseline');
touch('pointerup',2,230,120);
assert.equal(events.at(-1).type, 'pointerup');
assert.equal(touch('pointermove',1,120,100).defaultPrevented, true, 'remaining finger cannot jump the camera');
touch('pointerup',1,120,100);
assert.equal(touch('pointerdown',4,120,100).defaultPrevented, false, 'next single-finger gesture works');
touch('pointerdown',5,220,100);
surface.dispatchEvent(new Event('blur'));
assert.equal(events.at(-1).type, 'pointerup');
assert.equal(touch('pointerdown',6,900,900).defaultPrevented, false, 'outside viewport untouched');
dispose();
console.log('Multi-touch navigation tests passed');
