struct View { origin: vec2<f32>, extent: vec2<f32> }
struct Instance { position: vec2<f32>, size: vec2<f32>, atlas: vec2<u32>, base: i32, padding: u32 }
@group(0) @binding(0) var<uniform> camera: View;
@group(0) @binding(1) var colors: texture_2d<f32>;
@group(0) @binding(2) var depths: texture_2d<u32>;
@group(0) @binding(3) var<storage,read> instances: array<Instance>;
const corners = array<vec2<f32>,6>(vec2(0.,0.),vec2(1.,0.),vec2(0.,1.),vec2(0.,1.),vec2(1.,0.),vec2(1.,1.));
struct TerrainVertex { @builtin(position) position:vec4<f32>, @location(0) uv:vec2<f32>, @location(1) @interpolate(flat) base:i32 }
@vertex fn terrain_vertex(@builtin(vertex_index) v:u32,@builtin(instance_index) i:u32)->TerrainVertex {
    let inst=instances[i];let p=corners[v];
    let ndc=(inst.position+p*inst.size-camera.origin)/camera.extent;
    var out:TerrainVertex;out.position=vec4(ndc.x*2.-1.,1.-ndc.y*2.,0.,1.);
    out.uv=vec2<f32>(inst.atlas)+p*inst.size;out.base=inst.base;return out;
}
struct TerrainPixel { @location(0) color:vec4<f32>, @builtin(frag_depth) depth:f32 }
@fragment fn terrain_pixel(input:TerrainVertex)->TerrainPixel {
    let pixel=vec2<i32>(floor(input.uv));let color=textureLoad(colors,pixel,0);
    if color.a<0.5 {discard;}
    var out:TerrainPixel;out.color=color;
    let word=(input.base+i32(textureLoad(depths,pixel,0).r))&65535;
    out.depth=f32(word)/65535.;return out;
}
@group(1) @binding(0) var rendered: texture_2d<f32>;
struct QuadVertex { @builtin(position) position:vec4<f32>, @location(0) uv:vec2<f32> }
@vertex fn quad_vertex(@builtin(vertex_index) v:u32)->QuadVertex {
    var out:QuadVertex;let p=corners[v];out.position=vec4(p.x*2.-1.,1.-p.y*2.,0.,1.);out.uv=p;return out;
}
fn linear(c:vec3<f32>)->vec3<f32> {return select(c/12.92,pow((c+0.055)/1.055,vec3(2.4)),c>vec3(0.04045));}
@fragment fn quad_pixel(input:QuadVertex)->@location(0) vec4<f32> {
    let size=textureDimensions(rendered);let pos=min(vec2<u32>(input.uv*vec2<f32>(size)),size-vec2<u32>(1));
    let color=textureLoad(rendered,vec2<i32>(pos),0);
    if TARGET_SRGB {return vec4(linear(color.rgb),color.a);}return color;
}
