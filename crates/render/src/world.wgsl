struct View { origin:vec2<f32>, extent:vec2<f32>, flags:vec4<u32> }
struct Instance { position:vec2<f32>, size:vec2<f32>, atlas:vec2<u32>, base:i32, layer:u32, tint:vec4<f32> }
@group(0) @binding(0) var<uniform> camera:View;
@group(0) @binding(1) var colors:texture_2d<f32>;
@group(0) @binding(2) var originals:texture_2d<f32>;
@group(0) @binding(3) var depths:texture_2d<u32>;
@group(0) @binding(4) var<storage,read> instances:array<Instance>;
@group(0) @binding(5) var<storage,read> visible:array<u32>;
const corners=array<vec2<f32>,6>(vec2(0.,0.),vec2(1.,0.),vec2(0.,1.),vec2(0.,1.),vec2(1.,0.),vec2(1.,1.));
struct Vertex { @builtin(position) position:vec4<f32>, @location(0) uv:vec2<f32>, @location(1) @interpolate(flat) base:i32, @location(2) @interpolate(flat) layer:u32, @location(3) tint:vec3<f32> }
@vertex fn vs(@builtin(vertex_index) v:u32,@builtin(instance_index) i:u32)->Vertex {
 let inst=instances[visible[i]];let p=corners[v];let ndc=(inst.position+p*inst.size-camera.origin)/camera.extent;
 var out:Vertex;out.position=vec4(ndc.x*2.-1.,1.-ndc.y*2.,0.,1.);out.uv=vec2<f32>(inst.atlas)+p*inst.size;out.base=inst.base;out.layer=inst.layer;out.tint=inst.tint.rgb;return out;
}
struct Pixel { @location(0) color:vec4<f32>, @builtin(frag_depth) depth:f32 }
@fragment fn fs(input:Vertex)->Pixel {
 if (input.layer==1u&&camera.flags.x==0u)||(input.layer==2u&&camera.flags.y==0u){discard;}
 let p=vec2<i32>(floor(input.uv));var color=textureLoad(colors,p,0);if camera.flags.w==0u{color=textureLoad(originals,p,0);}
 if color.a==0. {discard;}if camera.flags.z!=0u{color=vec4(clamp(color.rgb*input.tint,vec3(0.),vec3(1.)),color.a);}
 var out:Pixel;out.color=color;out.depth=f32(clamp(input.base+i32(textureLoad(depths,p,0).r),0,65535))/65535.;return out;
}
