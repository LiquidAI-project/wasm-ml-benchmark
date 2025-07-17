use image::{ImageBuffer, Pixel, Rgba};
use libc::mode_t;
use ndarray::s;
use std::error::Error;
use std::fs;
use std::{
    cmp::Ordering,
    collections::HashMap,
    fmt::Debug,
    num::NonZero,
    ops::RangeFrom,
    time::{Duration, Instant},
};
use wasi_nn::{ExecutionTarget, Graph, GraphBuilder, GraphEncoding, GraphExecutionContext};

#[derive(Debug, Clone)]
struct Metrics {
    name: String,
    timestamp: Instant,
    wall_clock_time: Duration,
    user_time: Duration,
    system_time: Duration,
    max_rss: u64,
    cpu_usage: f32,
}

impl Metrics {
    fn current(name: String) -> Self {
        unsafe {
            let mut usage: rusage = std::mem::zeroed();
            usage.ru_utime.tv_sec = 1;
            usage.ru_utime.tv_usec = 0;
            usage.ru_stime.tv_sec = 1;
            usage.ru_stime.tv_usec = 0;

            // getrusage(0, &mut usage);

            let user_time: Duration = Duration::from_secs(usage.ru_utime.tv_sec as u64)
                + Duration::from_micros(usage.ru_utime.tv_usec as u64);

            let system_time: Duration = Duration::from_secs(usage.ru_stime.tv_sec as u64)
                + Duration::from_micros(usage.ru_stime.tv_usec as u64);

            let cpu_usage: f32 = 0.0;
            Self {
                name,
                timestamp: Instant::now(),
                wall_clock_time: Duration::default(),
                user_time,
                system_time,
                max_rss: 0 as u64,
                cpu_usage,
            }
        }
    }

    fn diff(&self, prev: &Self) -> Self {
        let wall_clock_time: Duration = self.timestamp.duration_since(prev.timestamp);
        let user_time: Duration = self.user_time - prev.user_time;
        let system_time: Duration = self.system_time - prev.system_time;

        let cpu_usage: f32 = if wall_clock_time.as_secs_f32() > 0.0 {
            let cpu_time: f32 = (user_time + system_time).as_secs_f32();
            (cpu_time / wall_clock_time.as_secs_f32()) * 100.0
        } else {
            0.0
        };

        Self {
            name: self.name.clone(),
            timestamp: self.timestamp,
            wall_clock_time,
            user_time,
            system_time,
            max_rss: self.max_rss - prev.max_rss,
            cpu_usage,
        }
    }

    fn combine(&self, other: &Self) -> Self {
        let combined_wall_clock = self.wall_clock_time + other.wall_clock_time;
        let combined_user_time = self.user_time + other.user_time;
        let combined_system_time = self.system_time + other.system_time;

        let cpu_usage = if combined_wall_clock.as_secs_f32() > 0.0 {
            let cpu_time = (combined_user_time + combined_system_time).as_secs_f32();
            (cpu_time / combined_wall_clock.as_secs_f32()) * 100.0
        } else {
            0.0
        };

        Self {
            name: self.name.clone(),
            timestamp: self.timestamp,
            wall_clock_time: combined_wall_clock,
            user_time: combined_user_time,
            system_time: combined_system_time,
            max_rss: self.max_rss.max(other.max_rss),
            cpu_usage,
        }
    }
}

impl std::fmt::Display for Metrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "============= {} Metrics =============", self.name)?;
        writeln!(f, "Wall Clock Time: {:?}", self.wall_clock_time)?;
        writeln!(f, "User time: {:?}", self.user_time)?;
        writeln!(f, "System time: {:?}", self.system_time)?;
        writeln!(f, "Max RSS: {} bytes", self.max_rss)?;
        writeln!(f, "CPU Usage: {}%", self.cpu_usage)?;
        writeln!(f, "=======================================")
    }
}

#[derive(Debug)]
struct BenchmarkTracker {
    start_metrics: Metrics,
    current_operation: Option<Metrics>,
    completed_metrics: Vec<Metrics>,
    active_phases: HashMap<String, Metrics>,
    phase_metrics: Vec<(String, Metrics)>,
    phase_order: Vec<String>,
}

impl BenchmarkTracker {
    fn new() -> Self {
        Self {
            start_metrics: Metrics::current("Total".to_string()),
            current_operation: None,
            completed_metrics: Vec::new(),
            active_phases: HashMap::new(),
            phase_metrics: Vec::new(),
            phase_order: Vec::new(),
        }
    }

    fn start_operation(&mut self, name: &str) {
        self.current_operation = Some(Metrics::current(name.to_string()));
    }

    fn finish_operation(&mut self) {
        if let Some(start_metrics) = self.current_operation.take() {
            self.finish_operation_internal(start_metrics);
        }
    }

    fn finish_operation_internal(&mut self, start_metrics: Metrics) {
        let end_metrics: Metrics = Metrics::current(start_metrics.name.clone());
        let diff_metrics: Metrics = end_metrics.diff(&start_metrics);

        self.completed_metrics.push(diff_metrics.clone());

        for (_, phase_metrics) in self.active_phases.iter_mut() {
            *phase_metrics = phase_metrics.combine(&diff_metrics);
        }
    }

    fn start_phase(&mut self, phase_name: &str) {
        let zero_metrics = Metrics {
            name: phase_name.to_string(),
            timestamp: Instant::now(),
            wall_clock_time: Duration::default(),
            user_time: Duration::default(),
            system_time: Duration::default(),
            max_rss: 0,
            cpu_usage: 0.0,
        };

        self.active_phases
            .insert(phase_name.to_string(), zero_metrics);

        if !self.phase_order.contains(&phase_name.to_string()) {
            self.phase_order.push(phase_name.to_string());
        }
    }

    fn end_phase(&mut self, phase_name: &str) {
        if let Some(metrics) = self.active_phases.remove(phase_name) {
            self.phase_metrics.push((phase_name.to_string(), metrics));
        }
    }

    fn get_total_metrics(&self) -> Metrics {
        let current: Metrics = Metrics::current("Total".to_string());
        current.diff(&self.start_metrics)
    }

    fn print_all_metrics(&self) {
        let total: Metrics = self.get_total_metrics();

        for metrics in &self.completed_metrics {
            print!("{}", metrics);
        }

        if !self.phase_metrics.is_empty() {
            println!("\n=========== Phase Metrics ===========");

            let group_map: HashMap<String, &Metrics> = self
                .phase_metrics
                .iter()
                .map(|(name, metrics)| (name.clone(), metrics))
                .collect();

            for phase_name in &self.phase_order {
                if let Some(metrics) = group_map.get(phase_name) {
                    print!("{}", metrics);
                }
            }
            println!("====================================\n");
        }

        print!("{}", total);
    }
}

fn initialize_env(model: &Graph) -> Result<GraphExecutionContext<'_>, Box<dyn Error>> {
    match model.init_execution_context() {
        Ok(context) => Ok(context),
        Err(_) => Err("Error occured while initializing the env".into()),
    }
}

fn load_model(model_path: &str) -> Result<Graph, wasi_nn::Error> {
    GraphBuilder::new(GraphEncoding::Onnx, ExecutionTarget::CPU).build_from_files([model_path])
}

fn read_img(image_path: &str) -> Result<ImageBuffer<Rgba<u8>, Vec<u8>>, Box<dyn Error>> {
    const IMAGE_WIDTH: u32 = 224;
    const IMAGE_HEIGHT: u32 = 224;

    let image = image::imageops::resize(
        &image::open(image_path)?,
        IMAGE_WIDTH,
        IMAGE_HEIGHT,
        image::imageops::FilterType::Triangle,
    );

    Ok(image)
}

pub fn image_to_tensor(image: ImageBuffer<Rgba<u8>, Vec<u8>>) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut array = ndarray::Array::from_shape_fn((1, 3, 224, 224), |(_, c, j, i)| {
        let pixel = image.get_pixel(i as u32, j as u32);
        let channels = pixel.channels();

        // range [0, 255] -> range [0, 1]
        (channels[c] as f32) / 255.0
    });

    let mean = [0.485, 0.456, 0.406];
    let std = [0.229, 0.224, 0.225];
    for c in 0..3 {
        let mut channel_array = array.slice_mut(s![0, c, .., ..]);
        channel_array -= mean[c];
        channel_array /= std[c];
    }

    Ok(f32_vec_to_bytes(array.as_slice().unwrap().to_vec()))
}

fn f32_vec_to_bytes(data: Vec<f32>) -> Vec<u8> {
    let chunks: Vec<[u8; 4]> = data.into_iter().map(|f| f.to_le_bytes()).collect();
    let mut result: Vec<u8> = Vec::new();

    for c in chunks {
        for u in c.iter() {
            result.push(*u);
        }
    }
    result
}

fn process_image(image_path: ImageBuffer<Rgba<u8>, Vec<u8>>) -> Result<Vec<u8>, Box<dyn Error>> {
    image_to_tensor(image_path)
}

fn run_model(context: &mut GraphExecutionContext) -> Result<(), Box<dyn Error>> {
    context.compute().map_err(|_| {
        Box::<dyn std::error::Error>::from("Error occurred while running the model")
    })?;
    Ok(())
}

fn post_process(
    context: &mut GraphExecutionContext,
    image_name: &str,
) -> Result<i32, Box<dyn Error>> {
    const OUTPUT_BUFFER_CAPACITY: usize = 4000;
    let mut output_buffer: Vec<f32> = vec![0.0; OUTPUT_BUFFER_CAPACITY];
    let context = context;

    match context.get_output(0, &mut output_buffer) {
        Ok(_) => (),
        Err(_) => return Err("Error occurred while getting output".into()),
    }

    let result = output_buffer
        .iter()
        .cloned()
        .zip(RangeFrom::<i32> { start: 1 })
        .max_by(|(score1, _), (score2, _)| score1.partial_cmp(score2).unwrap_or(Ordering::Equal))
        .map_or_else(|| Err("testing"), Ok);

    match result {
        Ok((score, class)) => {
            println!("{}: {} (score: {})", image_name, class, score);
            Ok(class)
        }
        Err(error) => {
            println!("Error: {:?}", error);
            Err("Error: ".into())
        }
    }
}

#[no_mangle]
pub fn main() {
    // let args: Vec<String> = env::args().collect();

    // if args.len() != 3 {
    //     return Err(format!("Usage: {} <model> <image>", args[0]).into());
    // }

    let model_path: String = String::from("/assets/models/mobilenetv2-10.onnx");
    let image_path: String = String::from("/assets/imgs/unseen_dog.jpg");

    let mut tracker: BenchmarkTracker = BenchmarkTracker::new();

    // RED BOX: Environment setup, image loading, processing, and model loading
    tracker.start_phase("RED BOX Phase");

    tracker.start_operation("loadmodel");
    let model: Result<Graph, wasi_nn::Error> = load_model(model_path.as_str());
    let model = model.unwrap();
    tracker.finish_operation();

    tracker.start_operation("envload");
    let mut context: GraphExecutionContext<'_> = initialize_env(&model).unwrap();
    tracker.finish_operation();

    tracker.start_operation("readimg");
    let original_img: ImageBuffer<Rgba<u8>, Vec<u8>> = read_img(image_path.as_str()).unwrap();
    tracker.finish_operation();

    tracker.end_phase("RED BOX Phase");

    // GREEN BOX: Model inference and post-processing
    tracker.start_phase("GREEN BOX Phase");

    tracker.start_operation("Pre-processing");
    let input = process_image(original_img).unwrap();
    context.set_input(0, wasi_nn::TensorType::F32, &[1, 3, 224, 224], &input);
    tracker.finish_operation();

    tracker.start_operation("Inference");
    let _ = run_model(&mut context);
    tracker.finish_operation();

    tracker.start_operation("Post-processing");
    let output: i32 = post_process(&mut context, image_path.as_str()).unwrap();
    tracker.finish_operation();

    tracker.end_phase("GREEN BOX Phase");

    tracker.print_all_metrics();

    println!("Predicted Class Index: {}", output);

    // let number_threads: NonZero<usize> = num_threads().unwrap();
    // println!("Number of Threads: {:?}", number_threads);
}
