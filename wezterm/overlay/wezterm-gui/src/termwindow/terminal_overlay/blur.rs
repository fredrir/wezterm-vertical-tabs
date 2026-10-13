//! Two separable GPU blur passes below the quick terminal; the second restores outside its frame.
use super::*;
use crate::renderstate::RenderContext;
use crate::termwindow::webgpu::WebGpuState;
use std::cell::{Cell, RefMut};
use wgpu::util::DeviceExt;
use window::glium::{self, Surface};

#[derive(Clone, Copy)]
pub(crate) struct BlurParams {
    bounds: Bounds,
    radius: f32,
    corner: f32,
}

pub(crate) struct BackdropBlur {
    context: RenderContext,
    size: (u32, u32),
    backend: Backend,
}

enum Backend {
    Gl {
        scene: glium::texture::SrgbTexture2d,
        scratch: glium::texture::SrgbTexture2d,
        program: glium::Program,
    },
    WebGpu(WebGpuBlur),
}

/// Layers below the quick terminal draw into the blur's scene, composited before the layers above.
pub(crate) struct Backdrop<'a> {
    blur: Option<(BlurParams, RefMut<'a, BackdropBlur>)>,
    composited: Cell<bool>,
}

impl<'a> Backdrop<'a> {
    fn new(blur: Option<(BlurParams, RefMut<'a, BackdropBlur>)>) -> Self {
        Self {
            blur,
            composited: Cell::new(false),
        }
    }

    fn below(&self, zindex: i8) -> bool {
        if !self.composited.get() && zindex >= ZINDEX {
            self.composited.set(true);
        }
        !self.composited.get()
    }

    /// Cleared alongside the frame.
    pub(crate) fn gl_scene(
        &self,
    ) -> anyhow::Result<Option<glium::framebuffer::SimpleFrameBuffer<'_>>> {
        let Some((_, blur)) = &self.blur else {
            return Ok(None);
        };
        let mut scene = blur.gl_scene()?;
        scene.clear_color(0., 0., 0., 0.);
        Ok(Some(scene))
    }

    pub(crate) fn gl_target<'t, 's>(
        &self,
        zindex: i8,
        frame: &'t mut glium::Frame,
        scene: &'t mut Option<glium::framebuffer::SimpleFrameBuffer<'s>>,
    ) -> anyhow::Result<GlTarget<'t, 's>> {
        let composited = self.composited.get();
        if self.below(zindex) {
            if let Some(scene) = scene {
                return Ok(GlTarget::Scene(scene));
            }
        } else if !composited {
            if let Some((params, blur)) = &self.blur {
                blur.draw_gl(frame, *params)?;
            }
        }
        Ok(GlTarget::Frame(frame))
    }

    /// Where a layer at `zindex` draws; the blur fills `view` when the first layer above begins.
    pub(crate) fn view<'v>(
        &'v self,
        zindex: i8,
        gpu: &WebGpuState,
        encoder: &mut wgpu::CommandEncoder,
        view: &'v wgpu::TextureView,
        cleared: &mut bool,
    ) -> &'v wgpu::TextureView {
        let composited = self.composited.get();
        let below = self.below(zindex);
        match &self.blur {
            Some((_, blur)) if below => blur.webgpu_scene(),
            Some((params, blur)) if !composited => {
                blur.draw_webgpu(gpu, encoder, view, *params);
                *cleared = true;
                view
            }
            _ => view,
        }
    }
}

pub(crate) enum GlTarget<'t, 's> {
    Frame(&'t mut glium::Frame),
    Scene(&'t mut glium::framebuffer::SimpleFrameBuffer<'s>),
}

impl GlTarget<'_, '_> {
    pub(crate) fn draw<'a, 'b, V, I, U>(
        &mut self,
        vertices: V,
        indices: I,
        program: &glium::Program,
        uniforms: &U,
        parameters: &glium::DrawParameters<'_>,
    ) -> Result<(), glium::DrawError>
    where
        V: glium::vertex::MultiVerticesSource<'b>,
        I: Into<glium::index::IndicesSource<'a>>,
        U: glium::uniforms::Uniforms,
    {
        match self {
            Self::Frame(frame) => frame.draw(vertices, indices, program, uniforms, parameters),
            Self::Scene(scene) => scene.draw(vertices, indices, program, uniforms, parameters),
        }
    }
}

impl TermWindow {
    pub(crate) fn prepare_terminal_blur(&self) -> anyhow::Result<Backdrop<'_>> {
        let (Some(ui), Some(config)) = (&self.ui_host, self.terminal_overlay_config()) else {
            return Ok(Backdrop::new(None));
        };
        if !self.terminal_overlay_visible() || config.opacity >= 1. || config.blur <= 0. {
            // Release the full-window textures when no longer needed.
            ui.terminal.blur.borrow_mut().take();
            return Ok(Backdrop::new(None));
        }
        let size = (
            self.dimensions.pixel_width as u32,
            self.dimensions.pixel_height as u32,
        );
        if size.0 == 0 || size.1 == 0 {
            return Ok(Backdrop::new(None));
        }
        let context = &self.render_state.as_ref().unwrap().context;
        let mut cache = ui.terminal.blur.borrow_mut();
        let matches = cache.as_ref().is_some_and(|blur| {
            blur.size == size
                && match (&blur.context, context) {
                    (RenderContext::Glium(a), RenderContext::Glium(b)) => Rc::ptr_eq(a, b),
                    (RenderContext::WebGpu(a), RenderContext::WebGpu(b)) => Rc::ptr_eq(a, b),
                    _ => false,
                }
        });
        if !matches {
            *cache = Some(BackdropBlur::new(context, size)?);
        }
        let scale = self.dimensions.dpi as f32 / 96.;
        let bounds = self.terminal_overlay_bounds();
        Ok(Backdrop::new(Some((
            BlurParams {
                bounds,
                radius: config.blur * scale,
                corner: corner_radius(self.dimensions.dpi as u32)
                    .min(bounds.width / 2.)
                    .min(bounds.height / 2.),
            },
            RefMut::map(cache, |cache| cache.as_mut().unwrap()),
        ))))
    }
}

impl BackdropBlur {
    fn new(context: &RenderContext, size: (u32, u32)) -> anyhow::Result<Self> {
        let backend = match context {
            RenderContext::Glium(gl) => Backend::Gl {
                scene: glium::texture::SrgbTexture2d::empty(gl, size.0, size.1)?,
                scratch: glium::texture::SrgbTexture2d::empty(gl, size.0, size.1)?,
                program: {
                    let mut errors = Vec::new();
                    let mut program = None;
                    for version in ["330 core", "330", "320 es", "300 es"] {
                        match glium::Program::from_source(
                            gl,
                            &format!("#version {version}\n{GL_VERTEX}"),
                            &format!("#version {version}\n{GL_FRAGMENT}"),
                            None,
                        ) {
                            Ok(compiled) => {
                                program = Some(compiled);
                                break;
                            }
                            Err(error) => errors.push(format!("{version}: {error}")),
                        }
                    }
                    program.ok_or_else(|| {
                        anyhow::anyhow!("Quick terminal blur shader: {}", errors.join("; "))
                    })?
                },
            },
            RenderContext::WebGpu(gpu) => Backend::WebGpu(WebGpuBlur::new(
                &gpu.device,
                gpu.config.borrow().format,
                size,
            )),
        };
        Ok(Self {
            context: context.clone(),
            size,
            backend,
        })
    }

    pub(crate) fn gl_scene(&self) -> anyhow::Result<glium::framebuffer::SimpleFrameBuffer<'_>> {
        match (&self.context, &self.backend) {
            (RenderContext::Glium(gl), Backend::Gl { scene, .. }) => {
                Ok(glium::framebuffer::SimpleFrameBuffer::new(gl, scene)?)
            }
            _ => unreachable!(),
        }
    }

    pub(crate) fn draw_gl(
        &self,
        output: &mut glium::Frame,
        params: BlurParams,
    ) -> anyhow::Result<()> {
        let (
            RenderContext::Glium(gl),
            Backend::Gl {
                scene,
                scratch,
                program,
            },
        ) = (&self.context, &self.backend)
        else {
            unreachable!()
        };
        let sampler = |texture| {
            glium::uniforms::Sampler::new(texture)
                .wrap_function(glium::uniforms::SamplerWrapFunction::Clamp)
                .magnify_filter(glium::uniforms::MagnifySamplerFilter::Linear)
                .minify_filter(glium::uniforms::MinifySamplerFilter::Linear)
        };
        let bounds = params.bounds;
        for final_pass in [false, true] {
            let direction = if final_pass {
                (0., params.radius / 8. / self.size.1 as f32)
            } else {
                (params.radius / 8. / self.size.0 as f32, 0.)
            };
            let uniforms = glium::uniform! {
                source: sampler(if final_pass { scratch } else { scene }),
                original: sampler(scene),
                direction: direction,
                bounds: (bounds.x, bounds.y, bounds.width, bounds.height),
                window_size: (self.size.0 as f32, self.size.1 as f32),
                corner: params.corner,
                final_pass: final_pass,
            };
            let vertices = glium::vertex::EmptyVertexAttributes { len: 3 };
            let indices = glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList);
            if final_pass {
                output.draw(vertices, indices, program, &uniforms, &Default::default())?;
            } else {
                glium::framebuffer::SimpleFrameBuffer::new(gl, scratch)?.draw(
                    vertices,
                    indices,
                    program,
                    &uniforms,
                    &Default::default(),
                )?;
            }
        }
        Ok(())
    }

    pub(crate) fn webgpu_scene(&self) -> &wgpu::TextureView {
        match &self.backend {
            Backend::WebGpu(gpu) => &gpu.scene,
            _ => unreachable!(),
        }
    }

    pub(crate) fn draw_webgpu(
        &self,
        gpu: &WebGpuState,
        encoder: &mut wgpu::CommandEncoder,
        output: &wgpu::TextureView,
        params: BlurParams,
    ) {
        let Backend::WebGpu(blur) = &self.backend else {
            unreachable!()
        };
        blur.draw(&gpu.device, encoder, output, params);
    }
}

struct WebGpuBlur {
    scene_texture: wgpu::Texture,
    scene: wgpu::TextureView,
    scratch: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
}

impl WebGpuBlur {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat, size: (u32, u32)) -> Self {
        let texture = |label| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Quick terminal blur"),
            source: wgpu::ShaderSource::Wgsl(WGSL.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Quick terminal blur"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let scene_texture = texture("Quick terminal backdrop");
        Self {
            scene: scene_texture.create_view(&Default::default()),
            scene_texture,
            scratch: texture("Quick terminal horizontal blur").create_view(&Default::default()),
            pipeline,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("Quick terminal blur"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
        }
    }

    fn draw(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        output: &wgpu::TextureView,
        params: BlurParams,
    ) {
        let Self {
            scene,
            scratch,
            pipeline,
            sampler,
            ..
        } = self;
        let bounds = params.bounds;
        for final_pass in [false, true] {
            let direction = if final_pass {
                [0., params.radius / 8. / self.scene_texture.height() as f32]
            } else {
                [params.radius / 8. / self.scene_texture.width() as f32, 0.]
            };
            let data = [
                direction[0],
                direction[1],
                params.corner,
                if final_pass { 1. } else { 0. },
                bounds.x,
                bounds.y,
                bounds.width,
                bounds.height,
                self.scene_texture.width() as f32,
                self.scene_texture.height() as f32,
                0.,
                0.,
            ];
            let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Quick terminal blur parameters"),
                contents: bytemuck::cast_slice(&data),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Quick terminal blur"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(if final_pass {
                            scratch
                        } else {
                            scene
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(scene),
                    },
                ],
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Quick terminal blur"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: if final_pass { output } else { scratch },
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

const GL_VERTEX: &str = r#"precision highp float;
out vec2 uv;
void main() {
    vec2 p = vec2((gl_VertexID << 1) & 2, gl_VertexID & 2);
    uv = p;
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
"#;
const GL_FRAGMENT: &str = r#"precision highp float;
uniform sampler2D source;
uniform sampler2D original;
uniform vec2 direction;
uniform vec4 bounds;
uniform vec2 window_size;
uniform float corner;
uniform bool final_pass;
in vec2 uv;
out vec4 color;
// The glyph renderer writes sRGB explicitly with framebuffer conversion disabled.
vec4 to_srgb(vec4 linear) {
    vec3 higher = 1.055 * pow(linear.rgb, vec3(1.0 / 2.4)) - 0.055;
    vec3 lower = linear.rgb * 12.92;
    return vec4(mix(higher, lower, lessThan(linear.rgb, vec3(0.0031308))), linear.a);
}
void main() {
    float mask = 1.0;
    if (final_pass) {
        vec2 pixel = vec2(gl_FragCoord.x, window_size.y - gl_FragCoord.y);
        vec2 q = abs(pixel - bounds.xy - bounds.zw * 0.5) - (bounds.zw * 0.5 - corner);
        float distance = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - corner;
        mask = clamp(0.5 - distance, 0.0, 1.0);
        if (mask == 0.0) { color = to_srgb(texture(original, uv)); return; }
    }
    vec4 sum = vec4(0.0);
    float total = 0.0;
    for (int i = -8; i <= 8; i++) {
        float weight = exp(-0.5 * float(i * i) / 9.0);
        sum += texture(source, uv + direction * float(i)) * weight;
        total += weight;
    }
    color = to_srgb(final_pass ? mix(texture(original, uv), sum / total, mask) : sum / total);
}
"#;
const WGSL: &str = r#"
struct Params { direction: vec4<f32>, bounds: vec4<f32>, size: vec4<f32> }
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var filtering: sampler;
@group(0) @binding(2) var<uniform> params: Params;
@group(0) @binding(3) var original: texture_2d<f32>;
struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> Vertex {
    let p = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var v: Vertex;
    v.position = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    v.uv = vec2<f32>(p.x, 1.0 - p.y);
    return v;
}
@fragment fn fs_main(v: Vertex) -> @location(0) vec4<f32> {
    var mask = 1.0;
    if (params.direction.w > 0.0) {
        let q = abs(v.position.xy - params.bounds.xy - params.bounds.zw * 0.5)
            - (params.bounds.zw * 0.5 - params.direction.z);
        let distance = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - params.direction.z;
        mask = clamp(0.5 - distance, 0.0, 1.0);
        if (mask == 0.0) { return textureSampleLevel(original, filtering, v.uv, 0.0); }
    }
    var sum = vec4<f32>(0.0);
    var total = 0.0;
    for (var i = -8; i <= 8; i += 1) {
        let weight = exp(-0.5 * f32(i * i) / 9.0);
        sum += textureSampleLevel(source, filtering, v.uv + params.direction.xy * f32(i), 0.0) * weight;
        total += weight;
    }
    if (params.direction.w > 0.0) {
        return mix(textureSampleLevel(original, filtering, v.uv, 0.0), sum / total, mask);
    }
    return sum / total;
}
"#;

#[cfg(test)]
#[path = "../../../tests/vtabs/terminal_blur.rs"]
mod vtabs_tests;
