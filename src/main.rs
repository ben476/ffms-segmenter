use ffms2::frame::{Frame, Resizers};
use ffms2::video::VideoSource;
use std::fs::File;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::{thread, time};
use structopt::StructOpt;
use y4m::{encode, Colorspace, Encoder, Frame as Y4MFrame, Ratio};

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
    /// Force overwriting of existing index file, if any
    #[structopt(short = "f", long = "force")]
    force: bool,
    /// The first frame to extract
    #[structopt(short = "b", long = "start", default_value = "0")]
    start: usize,
    /// The last frame to extract
    /// (0 means last frame)
    #[structopt(short = "e", long = "end", default_value = "0")]
    end: usize,
    /// Set FFmpeg verbosity level
    #[structopt(short = "v", long = "verbose", default_value = "0")]
    verbose: usize,
    /// Disable progress reporting
    #[structopt(short = "p", long = "progress")]
    progress: bool,
    /// Write timecodes for all video tracks to outputfile_track00.tc.txt
    #[structopt(short = "c", long = "timecodes")]
    timecodes: bool,
    /// Write keyframes for all video tracks to outputfile_track00.kf.txt
    #[structopt(short = "k", long = "keyframes")]
    keyframes: bool,
    /// Set the audio indexing mask to N
    /// (-1 means index all tracks, 0 means index none)
    #[structopt(short = "t", long = "index", default_value = "0")]
    index_mask: i64,
    /// Set audio decoding error handling
    #[structopt(short = "s", long = "audio-decoding", default_value = "0")]
    ignore_errors: usize,
    /// The file to be indexed
    #[structopt(parse(from_os_str))]
    input_file: PathBuf,
    /// The output file.
    /// If no output filename is specified, input_file.ffindex will be used
    #[structopt(parse(from_os_str))]
    output_file: PathBuf,
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

#[inline]
fn dump_filename(track: &Track, track_num: usize, cache_file: &Path, suffix: &str) -> PathBuf {
    if track.NumFrames() == 0 {
        return PathBuf::new();
    }

    if let TrackType::TYPE_VIDEO = track.TrackType() {
        let start = cache_file.to_str().unwrap();
        let filename = format!("{}_track{:02}{}", start, track_num, suffix);
        PathBuf::from(filename)
    } else {
        PathBuf::new()
    }
}

fn do_indexing(
    args: &CliArgs,
    // cache_file: &Path,
    ignore_errors: IndexErrorHandling,
) -> std::io::Result<()> {
    let mut progress = 0;

    // if cache_file.exists() && !args.force {
    //     panic!(
    //         "Error: index file already exists, \
    //          use -f if you are sure you want to overwrite it."
    //     );
    // }

    let indexer = Indexer::new(&args.input_file).unwrap();

    if args.progress {
        update_progress(0, 100, None);
        indexer.ProgressCallback(update_progress, &mut progress);
    }

    if args.index_mask == -1 {
        indexer.TrackTypeIndexSettings(TrackType::TYPE_AUDIO, 1);
    }

    for i in 0..64 {
        if ((args.index_mask >> i) & 1) != 0 {
            indexer.TrackIndexSettings(i, 1);
        }
    }

    let index = indexer.DoIndexing2(ignore_errors).unwrap();

    // if args.timecodes {
    //     print_progress!(args.progress, "Writing timecodes...");
    //     let num_tracks = index.NumTracks();
    //     for t in 0..num_tracks {
    //         let track = Track::TrackFromIndex(&index, t);
    //         let filename = dump_filename(&track, t, &cache_file, ".tc.txt");
    //         if !filename.to_str().unwrap().is_empty() && track.WriteTimecodes(&filename).is_err() {
    //             eprintln!(
    //                 "Failed to write timecodes file {}",
    //                 filename.to_str().unwrap()
    //             );
    //         }
    //     }
    //     print_progress!(args.progress, "Done.");
    // }

    // if args.keyframes {
    //     print_progress!(args.progress, "Writing keyframes...");
    //     let num_tracks = index.NumTracks();
    //     for t in 0..num_tracks {
    //         let track = Track::TrackFromIndex(&index, t);
    //         let filename = dump_filename(&track, t, &cache_file, ".kf.txt");
    //         if !filename.to_str().unwrap().is_empty() {
    //             let mut file = File::create(filename)?;
    //             write!(file, "# keyframe format v1\nfps 0\n")?;
    //             let frame_count = track.NumFrames();
    //             for f in 0..frame_count {
    //                 if track.FrameInfo(f).KeyFrame() != 0 {
    //                     writeln!(file, "{}", f)?;
    //                 }
    //             }
    //         }
    //     }
    //     print_progress!(args.progress, "Done.");
    // }

    print_progress!(args.progress, "Video indexed!");

    let video_track_id = index.FirstTrackOfType(TrackType::TYPE_VIDEO).unwrap();

    let video_track = Track::TrackFromIndex(&index, video_track_id);

    let ref mut video_source = VideoSource::new(
        &args.input_file,
        video_track_id,
        &index,
        8,
        video::SeekMode::SEEK_NORMAL,
    )
    .unwrap();

    let video_properties = video_source.GetVideoProperties();

    let prop_frame = Frame::GetFrame(video_source, 0).unwrap();

    println!("Pixel format: {}", prop_frame.ConvertedPixelFormat);

    let yuv420p = Frame::GetPixFmt("yuv420p");
    let yuyv422 = Frame::GetPixFmt("yuyv422");
    let rgb24 = Frame::GetPixFmt("rgb24");
    let yuv420p10le = Frame::GetPixFmt("yuv420p10le");

    let ref mut pix_fmts = vec![yuv420p, yuv420p10le];

    let width = prop_frame.EncodedWidth as usize;
    let height = prop_frame.EncodedHeight as usize;

    println!("Original width: {}", width);
    println!("Original height: {}", height);

    let scaled_width = prop_frame.ScaledWidth;
    let scaled_height = prop_frame.ScaledHeight;

    println!("Scaled width: {}", scaled_width);
    println!("Scaled height: {}", scaled_height);

    let framerate = Ratio {
        num: video_properties.FPSNumerator as usize,
        den: video_properties.FPSDenominator as usize,
    };

    // video_source
    //     .SetInputFormatV(1 as usize, video::ColorRanges::CR_MPEG, yuv420p as usize)
    //     .unwrap();

    // video_source
    //     .SetOutputFormatV2(pix_fmts, width, height, Resizers::RESIZER_BICUBIC)
    //     .unwrap();

    thread::sleep(time::Duration::from_millis(100));

    let prop_frame = Frame::GetFrame(video_source, 1).unwrap();

    let y4m_colorspace = {
        if prop_frame.ConvertedPixelFormat == yuv420p {
            Colorspace::C420
        } else if prop_frame.ConvertedPixelFormat == yuv420p10le {
            Colorspace::C420p10
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Unsupported colorspace: ".to_owned() + &prop_frame.ColorSpace.to_string(),
            ));
        }
    };

    let line_size = match y4m_colorspace {
        Colorspace::C420 => [width, width / 4, width / 4, 0],
        Colorspace::C420p10 => [width * 2, (width / 4) * 2, (width / 4) * 2, 0],
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Unsupported colorspace",
            ))
        }
    };

    println!("Line size: {:?}", line_size);

    let mut outfile = File::create(&args.output_file).unwrap();

    let mut encoder = encode(width, height, framerate)
        .with_colorspace(y4m_colorspace)
        .write_header(&mut outfile)
        .unwrap();

    // let raw_array: [*const u8; 4] = prop_frame.Data;
    // let slice_array: [&[u8]; 3] = unsafe {
    //     std::mem::transmute(raw_array)
    // };

    for i in 0..100 {
        let mut frame = Frame::GetFrame(video_source, i).unwrap();

        // frame.set_LineSize(&line_size);
        // Frame::set_LineSize(&mut frame, &line_size);

        frame.Linesize[1] /= 2;
        frame.Linesize[2] /= 2;

        // println!("Line size: {:?}", frame.Linesize);

        // frame.Linesize[1] = 480;
        // frame.Linesize[2] = 480;

        // println!("Frame colorspace: {}", frame.ColorSpace);

        let pixel_data: Vec<Option<&[u8]>> = frame.get_pixel_data();

        // println!("Y plane length: {:?}", pixel_data[0].unwrap().len());
        // println!("U plane length: {:?}", pixel_data[1].unwrap().len());
        // println!("V plane length: {:?}", pixel_data[2].unwrap().len());

        // save each plane to a file
        // let mut y_file = File::create(format!("y_plane_{}.bin", i)).unwrap();
        // y_file.write_all(pixel_data[0].unwrap()).unwrap();
        // let mut u_file = File::create(format!("u_plane_{}.bin", i)).unwrap();
        // u_file.write_all(pixel_data[1].unwrap()).unwrap();
        // let mut v_file = File::create(format!("v_plane_{}.bin", i)).unwrap();
        // v_file.write_all(pixel_data[2].unwrap()).unwrap();

        let frame = Y4MFrame::new(
            [
                pixel_data[0].unwrap(),
                pixel_data[1].unwrap(),
                pixel_data[2].unwrap(),
            ],
            None,
        );

        // get xor of all planes
        // let mut xor = 0;
        // for plane in pixel_data {
        //     if let Some(plane) = plane {
        //         for byte in plane {
        //             xor ^= *byte;
        //         }
        //     }
        // }

        // println!("XOR: {}", xor);

        // let mut outfile = File::create("out/".to_owned() + &i.to_string() + ".y4m").unwrap();

        // let mut encoder = encode(width, height, framerate)
        //     .with_colorspace(y4m_colorspace)
        //     .write_header(&mut outfile)
        //     .unwrap();

        encoder.write_frame(&frame).unwrap();
    }

    // print_progress!(args.progress, "Done.");

    Ok(())
}

fn main() {
    let args = CliArgs::from_args();

    println!("{:?}", Frame::GetPixFmt("yuv420p"));
    println!("{:?}", Frame::GetPixFmt("yuyv422"));
    println!("{:?}", Frame::GetPixFmt("rgb24"));
    println!("{:?}", Frame::GetPixFmt("yuv420p10le"));

    // if args.ignore_errors > 3 {
    //     panic!("Error: invalid audio decoding error handling mode");
    // }

    // let cache_file = if let Some(out) = &args.output_file {
    //     out.to_path_buf()
    // } else {
    //     let file_stem = args
    //         .input_file
    //         .as_path()
    //         .file_stem()
    //         .unwrap()
    //         .to_str()
    //         .unwrap();
    //     let filename = format!("{}.ffindex", file_stem);
    //     Path::new(&filename).to_path_buf()
    // };

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
