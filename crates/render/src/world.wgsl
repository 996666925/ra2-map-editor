struct View { origin:vec2<f32>, extent:vec2<f32>, flags:vec4<u32> }
struct Instance { position:vec2<f32>, size:vec2<f32>, atlas:vec2<u32>, base:i32, layer:u32, tint:vec4<f32> }
@group(0) @binding(0) var<uniform> camera:View;
@group(0) @binding(1) var colors:texture_2d<f32>;
@group(0) @binding(2) var originals:texture_2d<f32>;
@group(0) @binding(3) var depths:texture_2d<u32>;
@group(0) @binding(4) var<storage,read> instances:array<Instance>;
@group(0) @binding(5) var<storage,read> visible:array<u32>;
@group(0) @binding(6) var frameworks:texture_2d<f32>;
@group(0) @binding(7) var destination:texture_2d<f32>;
const corners=array<vec2<f32>,6>(vec2(0.,0.),vec2(1.,0.),vec2(0.,1.),vec2(0.,1.),vec2(1.,0.),vec2(1.,1.));
struct Vertex { @builtin(position) position:vec4<f32>, @location(0) uv:vec2<f32>, @location(1) @interpolate(flat) base:i32, @location(2) @interpolate(flat) layer:u32, @location(3) @interpolate(flat) tint:vec4<f32>, @location(4) @interpolate(flat) bounds:vec4<f32> }
@vertex fn vs(@builtin(vertex_index) v:u32,@builtin(instance_index) i:u32)->Vertex {
 let inst=instances[visible[i]];let p=corners[v];let ndc=(inst.position+p*inst.size-camera.origin)/camera.extent;
 var out:Vertex;out.position=vec4(ndc.x*2.-1.,1.-ndc.y*2.,0.,1.);out.uv=vec2<f32>(inst.atlas)+p*inst.size;out.base=inst.base;out.layer=inst.layer;out.tint=inst.tint;out.bounds=vec4(vec2<f32>(inst.atlas),vec2<f32>(inst.atlas)+inst.size-1.);return out;
}
// Integrate texel coverage over one screen pixel when magnifying framework art.
// Only a one-screen-pixel transition is softened, keeping enlarged digits sharp.
fn framework_color(input:Vertex, footprint:vec2<f32>, nearest:vec4<f32>)->vec4<f32> {
 if input.tint.w==0.||nearest.a!=1.||any(footprint>=vec2(1.)){return nearest;}
 let uv=input.uv-0.5;let lo=vec2<i32>(floor(uv));
 let weight=clamp((fract(uv)-0.5)/max(footprint,vec2(0.0001))+0.5,vec2(0.),vec2(1.));
 let low=vec2<i32>(input.bounds.xy);let high=vec2<i32>(input.bounds.zw);
 var a=textureLoad(frameworks,clamp(lo,low,high),0);
 var b=textureLoad(frameworks,clamp(lo+vec2(1,0),low,high),0);
 var c=textureLoad(frameworks,clamp(lo+vec2(0,1),low,high),0);
 var d=textureLoad(frameworks,clamp(lo+vec2(1,1),low,high),0);
 // Keep the original alpha/depth silhouette; never pull in transparent atlas gaps.
 if a.a!=1.{a=nearest;}if b.a!=1.{b=nearest;}if c.a!=1.{c=nearest;}if d.a!=1.{d=nearest;}
 return mix(mix(a,b,weight.x),mix(c,d,weight.x),weight.y);
}
struct Pixel { @location(0) color:vec4<f32>, @builtin(frag_depth) depth:f32 }
fn packed_word(rgb:vec3<f32>)->u32 {
 let b=vec3<u32>(round(clamp(rgb,vec3(0.),vec3(1.))*255.));
 return ((b.r>>3u)<<11u)|((b.g>>2u)<<5u)|(b.b>>3u);
}
fn packed_color(source:vec3<f32>,old:vec3<f32>,selector:u32)->vec3<f32> {
 let s=packed_word(source);let d=packed_word(old);var w=s;
 if selector==1u {w=(d>>1u)&0x7befu;}
 if selector==4u {w=((s>>1u)&0x7befu)+((d>>1u)&0x7befu);}
 if selector==2u {w=((s>>2u)&0x39e7u)*3u+((d>>2u)&0x39e7u);}
 if selector==6u {w=((s>>2u)&0x39e7u)+((d>>2u)&0x39e7u)*3u;}
 // Active engine RGB565 presentation uses these rounded 5/6-bit codebooks.
 let r=(((w>>11u)&31u)*255u+15u)/31u;
 let g=(((w>>5u)&63u)*255u+31u)/63u;
 let b=((w&31u)*255u+15u)/31u;
 return vec3<f32>(f32(r),f32(g),f32(b))/255.;
}
@fragment fn fs(input:Vertex)->Pixel {
 let footprint=fwidth(input.uv);
 let layer=input.layer&255u;let selector=input.layer>>8u;
 if (layer==1u&&camera.flags.x==0u)||(layer==2u&&camera.flags.y==0u){discard;}
 let p=vec2<i32>(floor(input.uv));let framework=(camera.flags.w&2u)!=0u&&layer==0u;
 var color=textureLoad(colors,p,0);if (camera.flags.w&1u)==0u{color=textureLoad(originals,p,0);}
 if framework {color=framework_color(input,footprint,textureLoad(frameworks,p,0));}
 if color.a==0. {discard;}if camera.flags.z!=0u&&!framework{color=vec4(clamp(color.rgb*input.tint.rgb,vec3(0.),vec3(1.)),color.a);}
 if selector!=0u {color=vec4(packed_color(color.rgb,textureLoad(destination,vec2<i32>(input.position.xy),0).rgb,selector),1.);}
 var out:Pixel;out.color=color;out.depth=f32(clamp(input.base+i32(textureLoad(depths,p,0).r),0,65535))/65535.;return out;
}
