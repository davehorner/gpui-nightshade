// Opt-in inspection of the native WebGPU descriptors used by the real WASM app.
import captured from './mesh-pipeline-data.js?v=1';
const normalize=s=>s.replace(/\s+/g,' ').trim();
const stable=value=>JSON.stringify(value,(_,v)=>v&&typeof v==='object'&&!Array.isArray(v)?Object.fromEntries(Object.keys(v).sort().map(k=>[k,v[k]])):v);
const digest=async text=>Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',new TextEncoder().encode(text))),v=>v.toString(16).padStart(2,'0')).join('');
function difference(expected,actual){let at=0;while(at<Math.min(expected.length,actual.length)&&expected[at]===actual[at])at++;return at===expected.length&&at===actual.length?null:{at,expected:expected.slice(Math.max(0,at-100),at+180),actual:actual.slice(Math.max(0,at-100),at+180)};}
export function installPipelineDiagnostics(record){
  const shaders=new WeakMap(),groups=new WeakMap(),layouts=new WeakMap(),devices=new WeakMap();
  const expectedSource=normalize(captured.source.replaceAll('@interpolate(linear)',''));
  const adapterRequest=GPUAdapter.prototype.requestDevice;
  GPUAdapter.prototype.requestDevice=async function(descriptor){
    const original=JSON.parse(JSON.stringify(descriptor||{}));
    if(new URLSearchParams(location.search).get('storageBuffers')==='16')descriptor={...descriptor,requiredLimits:{...descriptor?.requiredLimits,maxStorageBuffersPerShaderStage:Math.min(16,this.limits.maxStorageBuffersPerShaderStage)}};
    const device=await adapterRequest.call(this,descriptor);devices.set(device,{original,requested:JSON.parse(JSON.stringify(descriptor||{})),actualStorageBuffersPerShaderStage:device.limits.maxStorageBuffersPerShaderStage});return device;
  };
  const shader=GPUDevice.prototype.createShaderModule;
  GPUDevice.prototype.createShaderModule=function(d){const module=shader.call(this,d);shaders.set(module,d.code);return module;};
  const group=GPUDevice.prototype.createBindGroupLayout;
  GPUDevice.prototype.createBindGroupLayout=function(d){const layout=group.call(this,d);groups.set(layout,JSON.parse(JSON.stringify(d)));return layout;};
  const layout=GPUDevice.prototype.createPipelineLayout;
  GPUDevice.prototype.createPipelineLayout=function(d){const result=layout.call(this,d);layouts.set(result,Array.from(d.bindGroupLayouts,g=>groups.get(g)));return result;};
  const pipeline=GPUDevice.prototype.createRenderPipeline;
  let serial=0;
  GPUDevice.prototype.createRenderPipeline=function(d){
    if(d.label==='Opaque Mesh Pipeline'){
      const id=++serial,source=shaders.get(d.fragment.module)||'',vertexSource=shaders.get(d.vertex.module)||'';
      const descriptor=JSON.parse(JSON.stringify({...d,layout:layouts.get(d.layout),vertex:{...d.vertex,module:undefined},fragment:{...d.fragment,module:undefined}}));
      const info={id,sourceLength:source.length,vertexEqualsFragment:source===vertexSource,sourceDifference:difference(expectedSource,normalize(source)),descriptorDifference:difference(stable(captured.descriptor),stable(descriptor)),device:devices.get(this)};
      record({pipeline:info});
      Promise.all([digest(source),digest(stable(descriptor))]).then(([sourceHash,descriptorHash])=>record({pipeline:{...info,sourceHash,descriptorHash}})).catch(error=>record({pipelineDiagnosticError:String(error)}));
    }
    return pipeline.call(this,d);
  };
  record('Pipeline capture enabled before starting WASM');
}
