use super::*;

#[test]
fn backdrop_blur_softens_content_only_inside_the_rounded_frame() {
    smol::block_on(async {
        let instance = wgpu::Instance::default();
        let Ok(adapter) = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
        else {
            eprintln!("No headless GPU adapter; skipping blur pixel test");
            return;
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .unwrap();
        let size = wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        };
        // Linear and sRGB formats; asymmetric content catches upside-down sampling.
        let pixels: Vec<u8> = (0..64)
            .flat_map(|y| {
                (0..64).flat_map(move |x| {
                    let value = if (x / 4 + y / 8) % 2 == 0 { 255 } else { 0 };
                    [value, y * 4, 0, 255]
                })
            })
            .collect();
        for format in [
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        ] {
            let blur = WebGpuBlur::new(&device, format, (64, 64));
            queue.write_texture(
                blur.scene_texture.as_image_copy(),
                &pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(64),
                },
                size,
            );
            let output = device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 64 * 256,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = device.create_command_encoder(&Default::default());
            blur.draw(
                &device,
                &mut encoder,
                &output.create_view(&Default::default()),
                BlurParams {
                    bounds: Bounds {
                        x: 16.,
                        y: 16.,
                        width: 32.,
                        height: 32.,
                    },
                    radius: 12.,
                    corner: 8.,
                },
            );
            encoder.copy_texture_to_buffer(
                output.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: Some(64),
                    },
                },
                size,
            );
            let submitted = queue.submit([encoder.finish()]);
            let (send, receive) = std::sync::mpsc::channel();
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    send.send(result).unwrap()
                });
            device.poll(wgpu::PollType::wait_for(submitted)).unwrap();
            receive.recv().unwrap().unwrap();
            let result = buffer.slice(..).get_mapped_range();
            for (x, y) in [(0, 0), (8, 37), (55, 20), (16, 16), (47, 47)] {
                let i = (y * 64 + x) * 4;
                for channel in 0..4 {
                    assert!(
                        (i16::from(result[i + channel]) - i16::from(pixels[i + channel])).abs()
                            <= 1,
                        "outside frame changed at ({}, {}) for {:?}",
                        x,
                        y,
                        format
                    );
                }
            }
            for (x, y) in [(28, 28), (32, 32), (35, 35)] {
                let i = (y * 64 + x) * 4;
                assert!(
                    result[i] > 30 && result[i] < 230,
                    "stripe not blurred: {}",
                    result[i]
                );
                assert_eq!(result[i + 3], 255);
            }
        }
    });
}
