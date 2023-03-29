use ffms2::frame::Frame;
use ffms2::video::VideoSource;
use std::fs::File;
use std::path::PathBuf;
use std::time::Instant;
use std::{thread, time};
use structopt::StructOpt;
use y4m::{encode, Colorspace, Frame as Y4MFrame, Ratio};

use ffms2::index::*;
use ffms2::track::*;
use ffms2::*;

macro_rules! print_progress {
    ($cond:expr, $error:expr) => {
        if $cond {
            eprintln!($error);
        }
    };
}

#[derive(Debug, StructOpt)]
struct CliArgs {
    /// Set FFmpeg verbosity level
    #[structopt(short = "v", long = "verbose", default_value = "0")]
    verbose: usize,
    /// Disable progress reporting
    #[structopt(short = "p", long = "progress")]
    progress: bool,
    /// The file to be indexed
    #[structopt(parse(from_os_str))]
    input_file: PathBuf,
    // If errors should be ignored
    #[structopt(short = "e", long = "ignore-errors", default_value = "0")]
    ignore_errors: usize,
    /// The output folder.
    /// Default to "." if not specified
    #[structopt(parse(from_os_str))]
    output_folder: Option<PathBuf>,
}

fn update_progress(current: usize, total: usize, private: Option<&mut usize>) -> usize {
    let percentage = ((current as f32 / total as f32) * 100.0) as usize;

    if let Some(percent) = private {
        if percentage <= *percent {
            return 0;
        }
        *percent = percentage;
    }

    eprintln!("Indexing, please wait... {}%", percentage);
    0
}

fn do_indexing(args: &CliArgs, ignore_errors: IndexErrorHandling) -> std::io::Result<()> {
    let mut progress = 0;

    let indexer = Indexer::new(&args.input_file).unwrap();

    if args.progress {
        update_progress(0, 100, None);
        indexer.ProgressCallback(update_progress, &mut progress);
    }

    let index = indexer.DoIndexing2(ignore_errors).unwrap();

    print_progress!(args.progress, "Video indexed!");

    let video_track_id = index.FirstTrackOfType(TrackType::TYPE_VIDEO).unwrap();

    let ref mut video_source = VideoSource::new(
        &args.input_file,
        video_track_id,
        &index,
        8,
        video::SeekMode::SEEK_NORMAL,
    )
    .unwrap();

    let video_properties = video_source.GetVideoProperties();

    let total_frames = video_properties.NumFrames;

    let prop_frame = Frame::GetFrame(video_source, 0).unwrap();

    println!(
        "{} {} {} {} {}",
        prop_frame.EncodedWidth,
        prop_frame.EncodedHeight,
        total_frames,
        video_properties.FPSDenominator,
        video_properties.FPSNumerator
    );

    eprintln!("Pixel format: {}", prop_frame.ConvertedPixelFormat);

    let yuv420p = Frame::GetPixFmt("yuv420p");
    let yuv422p = Frame::GetPixFmt("yuv422p");
    let yuv420p10le = Frame::GetPixFmt("yuv420p10le");

    let width = prop_frame.EncodedWidth as usize;
    let height = prop_frame.EncodedHeight as usize;

    eprintln!("Original width: {}", width);
    eprintln!("Original height: {}", height);

    let scaled_width = prop_frame.ScaledWidth;
    let scaled_height = prop_frame.ScaledHeight;

    eprintln!("Scaled width: {}", scaled_width);
    eprintln!("Scaled height: {}", scaled_height);

    let framerate = Ratio {
        num: video_properties.FPSNumerator as usize,
        den: video_properties.FPSDenominator as usize,
    };

    video_source
        .SetInputFormatV(1 as usize, video::ColorRanges::CR_MPEG, yuv420p as usize)
        .unwrap();

    thread::sleep(time::Duration::from_millis(100));

    let prop_frame = Frame::GetFrame(video_source, 1).unwrap();

    eprintln!("Pixel format: {}", prop_frame.ConvertedPixelFormat);
    eprintln!("Colorspace: {}", prop_frame.ColorSpace);

    let y4m_colorspace = {
        if prop_frame.ConvertedPixelFormat == yuv420p {
            Colorspace::C420
        } else if prop_frame.ConvertedPixelFormat == yuv420p10le {
            Colorspace::C420p10
        } else if prop_frame.ConvertedPixelFormat == yuv422p {
            Colorspace::C422
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Unsupported colorspace: ".to_owned()
                    + &prop_frame.ConvertedPixelFormat.to_string(),
            ));
        }
    };

    let line_size = match y4m_colorspace {
        Colorspace::C420 => [width, width / 4, width / 4, 0],
        Colorspace::C420p10 => [width * 2, (width / 4) * 2, (width / 4) * 2, 0],
        Colorspace::C422 => [width, width / 2, width / 2, 0],
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Unsupported colorspace",
            ))
        }
    };

    eprintln!("Line size: {:?}", line_size);

    let mut input = String::new();
    loop {
        input.clear();
        let _ = std::io::stdin().read_line(&mut input);

        let now = Instant::now();

        input = input.trim().to_string();

        let start_end_str = input.split(" ").collect::<Vec<&str>>();

        let start = start_end_str[0].parse::<usize>().unwrap();
        let end = {
            let end = start_end_str[1].parse::<usize>().unwrap();

            if end as i32 > total_frames {
                total_frames as usize
            } else {
                end
            }
        };

        eprintln!("Input: {}", input);
        eprintln!("Reading segment {} to {}", start, end);

        // join args.output_folder and start and end
        // default to current directory
        let ref outpath = format!(
            "{}/{}-{}.y4m",
            match args.output_folder {
                Some(ref folder) => folder.to_str().unwrap(),
                None => ".",
            },
            start,
            end
        );

        let mut outfile = File::create(outpath).unwrap();

        let mut encoder = encode(width, height, framerate)
            .with_colorspace(y4m_colorspace)
            .write_header(&mut outfile)
            .unwrap();

        for i in start..end {
            let mut frame = Frame::GetFrame(video_source, i).unwrap();

            // Work around for bug in FFMS2 Rust bindings
            frame.Linesize[1] /= 2;
            frame.Linesize[2] /= 2;

            let pixel_data: Vec<Option<&[u8]>> = frame.get_pixel_data();

            let frame = Y4MFrame::new(
                [
                    pixel_data[0].unwrap(),
                    pixel_data[1].unwrap(),
                    pixel_data[2].unwrap(),
                ],
                None,
            );

            encoder.write_frame(&frame).unwrap();
        }

        eprintln!("Time taken: {:?}", now.elapsed());

        println!("{} {}", start, outpath);
    }
}

fn main() {
    let args = CliArgs::from_args();

    FFMS2::Init();

    let level = match args.verbose {
        0 => LogLevels::LOG_QUIET,
        1 => LogLevels::LOG_WARNING,
        2 => LogLevels::LOG_INFO,
        3 => LogLevels::LOG_VERBOSE,
        _ => LogLevels::LOG_DEBUG,
    };

    Log::SetLogLevel(level);

    let ignore_errors = match args.ignore_errors {
        0 => IndexErrorHandling::IEH_IGNORE,
        1 => IndexErrorHandling::IEH_STOP_TRACK,
        2 => IndexErrorHandling::IEH_CLEAR_TRACK,
        _ => IndexErrorHandling::IEH_ABORT,
    };

    do_indexing(&args, ignore_errors).unwrap();
}
