//! Standalone comparison of two adjustment stacks sharing raster tiles and one GPU compositor.
use std::{error::Error, hint::black_box, time::Instant};

use photocraft_color::{BlendMode, ColorMode, SampleType};
use photocraft_doc::adjust::CurvePoint;
use photocraft_doc::{Adjustment, Document, Layer, LayerContent, LayerMask, Size};
use photocraft_geom::Rect;
use photocraft_gpu::{Compositor, Stats};
use photocraft_raster::Surface;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn fixture(size: Size, posterize: bool) -> Document {
    let mut doc = Document::new("Independent stacks", size, ColorMode::Rgb, SampleType::U8);
    let bounds = doc.bounds();
    for i in 0..4 {
        let mut surface = Surface::new(doc.pixel_format());
        let bytes: Vec<u8> = (0..size.height)
            .flat_map(|y| {
                (0..size.width).flat_map(move |x| [(x + 31 * i) as u8, (y + 53 * i) as u8, ((x ^ y) + 73 * i) as u8, if i == 0 { 255 } else { (x + y) as u8 }])
            })
            .collect();
        surface.write_interleaved(bounds, &bytes);
        let mut layer = Layer::new(format!("Raster {i}"), LayerContent::Raster(surface));
        layer.blend = [BlendMode::Normal, BlendMode::Multiply, BlendMode::Screen, BlendMode::Normal][i as usize];
        if i == 2 {
            let mut mask = LayerMask::reveal_all();
            mask.surface.fill_rect(Rect::new(0, 0, bounds.x1 / 2, bounds.y1), &[0.35]);
            layer.mask = Some(mask);
        }
        doc.layers.push(layer);
    }
    if posterize {
        doc.layers.push(Layer::new("Posterize", LayerContent::Adjustment(Adjustment::Posterize { levels: 12 })));
    }
    doc.layers.push(Layer::new("Curves", LayerContent::Adjustment(Adjustment::identity_curves())));
    doc
}

fn curve(doc: &mut Document, middle: f32) {
    if let Some(Layer { content: LayerContent::Adjustment(Adjustment::Curves { master, .. }), .. }) = doc.layers.last_mut() {
        *master = vec![CurvePoint { input: 0.0, output: 0.0 }, CurvePoint { input: 0.5, output: middle }, CurvePoint { input: 1.0, output: 1.0 }];
    }
}

fn wait(device: &wgpu::Device) -> Result<()> {
    device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None })?;
    Ok(())
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    if values.len() % 2 == 0 { (values[mid - 1] + values[mid]) / 2.0 } else { values[mid] }
}

#[derive(Default)]
struct Samples {
    cpu: Vec<f64>,
    wall: Vec<f64>,
    submit: Vec<f64>,
    gpu: Vec<f64>,
    stats: Stats,
    uploads: Vec<f64>,
}

impl Samples {
    fn measure(&mut self, comp: &mut Compositor, device: &wgpu::Device, queue: &wgpu::Queue, doc: &Document) -> Result<()> {
        let start = Instant::now();
        black_box(photocraft_compose::flatten(doc));
        self.cpu.push(start.elapsed().as_secs_f64() * 1000.0);
        let start = Instant::now();
        self.stats = comp.render(device, queue, doc, doc.bounds(), |_, _| {})?;
        self.uploads.push(self.stats.bytes_uploaded as f64);
        self.submit.push(start.elapsed().as_secs_f64() * 1000.0);
        wait(device)?;
        self.wall.push(start.elapsed().as_secs_f64() * 1000.0);
        if let Some(timing) = comp.take_gpu_timing() {
            self.gpu.push(timing.ms);
        }
        Ok(())
    }

    fn report(self, name: &str) {
        let n = self.wall.len();
        let gpu = if self.gpu.is_empty() { "unavailable".into() } else { format!("{:.3} ms ({} samples)", median(self.gpu.clone()), self.gpu.len()) };
        println!(
            "{name}: n={n} CPU={:.3} ms GPU wall={:.3} ms host submit={:.3} ms GPU timestamps={gpu}; median upload={:.0} bytes, tiles={}, prefix_hits={}, slots={}",
            median(self.cpu),
            median(self.wall),
            median(self.submit),
            median(self.uploads),
            self.stats.tiles_uploaded,
            self.stats.prefix_hits,
            self.stats.slots,
        );
    }
}

fn compare(cpu: &[[f32; 4]], gpu: &[[f32; 4]]) -> Result<(f32, f64, usize, usize)> {
    if cpu.len() != gpu.len() || cpu.is_empty() {
        return Err("readback dimensions differ".into());
    }
    let mut max = 0.0f32;
    let mut sum = 0.0f64;
    let mut beyond = 0;
    let mut worst = 0;
    for (index, (a, b)) in cpu.iter().zip(gpu).enumerate() {
        for (&x, &y) in a.iter().zip(b) {
            let error = (x - y).abs();
            if !error.is_finite() {
                return Err("nonfinite pixel error".into());
            }
            if error > max {
                max = error;
                worst = index;
            }
            beyond += usize::from(error > 2.0 / 255.0);
            sum += f64::from(error);
        }
    }
    Ok((max, sum / (cpu.len() as f64 * 4.0), beyond, worst))
}

fn verify(comp: &mut Compositor, device: &wgpu::Device, queue: &wgpu::Queue, a: &Document, b: &Document) -> Result<()> {
    let mut fingerprint = None;
    for (name, doc) in [("A", a), ("B", b), ("A again", a)] {
        let cpu = photocraft_compose::flatten(doc);
        let (gpu, stats) = photocraft_gpu::render_to_vec_stats(comp, device, queue, doc, doc.bounds())?;
        let (max, mean, beyond, worst) = compare(&cpu.px, &gpu)?;
        println!(
            "{name}: max absolute channel error={max:.8}, mean={mean:.8}, channels beyond 2/255={beyond}, worst at ({}, {}): CPU={:?}, GPU={:?}, uploads={} bytes",
            worst % doc.size.width as usize,
            worst / doc.size.width as usize,
            cpu.px[worst],
            gpu[worst],
            stats.bytes_uploaded,
        );
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut hash = DefaultHasher::new();
        for pixel in &gpu {
            pixel.map(f32::to_bits).hash(&mut hash);
        }
        let hash = hash.finish();
        match name {
            "A" => fingerprint = Some(hash),
            "B" if fingerprint == Some(hash) => return Err("divergent views produced identical output".into()),
            "A again" if fingerprint != Some(hash) => return Err("alternating views changed A's output".into()),
            _ => {}
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let probe = std::env::args().any(|arg| arg == "--probe");
    let quick = !probe && std::env::args().any(|arg| arg == "--quick");
    let size = if quick { Size::new(768, 512) } else { Size::new(6000, 4000) };
    let start = Instant::now();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(
        instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default() }),
    )?;
    let timestamps = wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_features: adapter.features() & timestamps, ..Default::default() }))?;
    let format = Compositor::preferred_acc_format(&adapter);
    println!("{:?}, {format:?}, {}x{}, device startup={:.3} ms", adapter.get_info(), size.width, size.height, start.elapsed().as_secs_f64() * 1000.0);

    let posterize = probe || !std::env::args().any(|arg| arg == "--curves-only");
    let mut a = fixture(size, posterize);
    if probe {
        let mut comp = Compositor::try_new_with_format(&device, format)?;
        let rect = Rect::new(3002, 142, 3003, 143);
        let multiply_source = a.layers[1].clone();
        a.layers.pop();
        for stage in ["Posterize", "input", "Screen", "Multiply", "base"] {
            let cpu = photocraft_compose::render(&a, rect).px[0];
            let gpu = photocraft_gpu::render_to_vec(&mut comp, &device, &queue, &a, rect)?[0];
            println!("{stage}: CPU={cpu:?} GPU={gpu:?}, red scaled={:?}", [cpu[0] * 255.0, gpu[0] * 255.0]);
            a.layers.pop();
        }
        a.layers.push(multiply_source);
        a.layers[0].blend = BlendMode::Normal;
        let cpu = photocraft_compose::render(&a, rect).px[0];
        let gpu = photocraft_gpu::render_to_vec(&mut comp, &device, &queue, &a, rect)?[0];
        println!("Multiply source only: CPU={cpu:?} GPU={gpu:?}");
        return Ok(());
    }
    let mut b = a.clone();
    curve(&mut a, 0.35);
    curve(&mut b, 0.65);
    let mut creation = Vec::new();
    let mut cold = Samples::default();
    let mut warm = Samples::default();
    let mut adjustment = Samples::default();
    let mut paint = Samples::default();
    for i in 0..5 {
        let start = Instant::now();
        let mut comp = Compositor::try_new_with_format(&device, format)?;
        comp.measure_gpu_time(&device, &queue);
        wait(&device)?;
        creation.push(start.elapsed().as_secs_f64() * 1000.0);
        cold.measure(&mut comp, &device, &queue, &a)?;
        warm.measure(&mut comp, &device, &queue, &b)?;
        if warm.stats.bytes_uploaded != 0 {
            return Err("unchanged source pixels were uploaded when alternating views".into());
        }
        let check_region = Rect::new(0, 0, 32, 32);
        let before = if i == 4 {
            let mut check = Compositor::try_new_with_format(&device, format)?;
            let a_pixels = photocraft_gpu::render_to_vec(&mut check, &device, &queue, &a, check_region)?;
            let b_pixels = photocraft_gpu::render_to_vec(&mut check, &device, &queue, &b, check_region)?;
            Some((check, a_pixels, b_pixels))
        } else {
            None
        };
        curve(&mut a, 0.38 + i as f32 * 0.01);
        adjustment.measure(&mut comp, &device, &queue, &a)?;
        let edited = if let Some((mut check, a_before, b_before)) = before {
            let a_after = photocraft_gpu::render_to_vec(&mut check, &device, &queue, &a, check_region)?;
            let b_after = photocraft_gpu::render_to_vec(&mut check, &device, &queue, &b, check_region)?;
            if a_before == a_after || b_before != b_after {
                return Err("changing A's adjustment did not isolate the result".into());
            }
            Some((check, a_after, b_after))
        } else {
            None
        };
        let Some(surface) = a.layers[0].surface_mut() else { return Err("missing raster source".into()) };
        surface.fill_rect(Rect::new(0, 0, 32, 32), &[0.11, 0.27, 0.48, 1.0]);
        let source = a.layers[0].clone();
        let old = std::mem::replace(&mut b.layers[0], source);
        paint.measure(&mut comp, &device, &queue, &a)?;
        // B must see the changed shared tiles without uploading them again.
        let shared = comp.render(&device, &queue, &b, b.bounds(), |_, _| {})?;
        wait(&device)?;
        if shared.bytes_uploaded != 0 {
            return Err("the second view uploaded the already resident edit".into());
        }
        if let Some((mut check, a_before, b_before)) = edited {
            let a_after = photocraft_gpu::render_to_vec(&mut check, &device, &queue, &a, check_region)?;
            let b_after = photocraft_gpu::render_to_vec(&mut check, &device, &queue, &b, check_region)?;
            if a_before == a_after || b_before == b_after {
                return Err("a view failed to show the shared pixel edit".into());
            }
            println!("A-only adjustment and shared pixel edit isolation passed");
            println!("resident sources={} bytes, prefix={} bytes", comp.resident_bytes(), comp.composite_cache_bytes());
            verify(&mut comp, &device, &queue, &a, &b)?;
        }
        a.layers[0] = old.clone();
        b.layers[0] = old;
        curve(&mut a, 0.35);
    }
    println!("compositor setup median={:.3} ms", median(creation));
    cold.report("cold A");
    warm.report("resident B");
    adjustment.report("resident adjustment edit A");
    paint.report("shared pixel edit A (full canvas)");
    Ok(())
}
