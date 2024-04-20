// pub fn dump_frames(filename: &str) -> Result<(), ffmpeg::Error> {
//     ffmpeg::init().unwrap();

//     if let Ok(mut ictx) = input(&filename) {
//         let input = ictx
//             .streams()
//             .best(Type::Video)
//             .ok_or(ffmpeg::Error::StreamNotFound)?;
//         let video_stream_index = input.index();

//         let context_decoder =
//             ffmpeg::codec::context::Context::from_parameters(input.parameters()).unwrap();
//         let mut decoder = context_decoder.decoder().video().unwrap();

//         let mut scaler = Context::get(
//             decoder.format(),
//             decoder.width(),
//             decoder.height(),
//             Pixel::RGB24,
//             decoder.width(),
//             decoder.height(),
//             Flags::BILINEAR,
//         )?;

//         let mut frame_index = 0;

//         let mut receive_and_process_decoded_frames =
//             |decoder: &mut ffmpeg::decoder::Video| -> Result<(), ffmpeg::Error> {
//                 let mut fail_count = 0;
//                 let mut decoded = Video::empty();
//                 // loop {
//                 //     match decoder.receive_frame(&mut decoded) {
//                 //         Ok(_) => {
//                 //             println!("---receiving frame...");
//                 //             let mut rgb_frame = Video::empty();
//                 //             scaler.run(&decoded, &mut rgb_frame).unwrap();
//                 //             save_file(&rgb_frame, frame_index).unwrap();
//                 //             frame_index += 1;
//                 //         }
//                 //         Err(e) => {
//                 //             // Handle other errors as needed
//                 //             println!("received error: {:?}", e);
//                 //             break;
//                 //         }
//                 //     }
//                 // }
//                 while decoder.receive_frame(&mut decoded).is_ok() {
//                     println!("---receiving frame...");
//                     let mut rgb_frame = Video::empty();
//                     scaler.run(&decoded, &mut rgb_frame).unwrap();
//                     save_file(&rgb_frame, frame_index).unwrap();
//                     frame_index += 1;
//                 }
//                 Ok(())
//             };

//         for (stream, packet) in ictx.packets() {
//             println!(
//                 "-- receiving packet: {} | {:?}",
//                 stream.index(),
//                 packet.pts()
//             );
//             if stream.index() == video_stream_index {
//                 println!("--- got video packet...");
//                 match decoder.send_packet(&packet) {
//                     Ok(()) => receive_and_process_decoded_frames(&mut decoder).unwrap(),
//                     Err(err) => println!("received err in send_packet: {:?}", err),
//                 }
//             }
//         }
//         decoder.send_eof()?;
//         receive_and_process_decoded_frames(&mut decoder)?;
//     }

//     Ok(())
// }

// Code snippets that may be useful in the future for debugging
// fn save_file(frame: &Video, index: usize) -> std::result::Result<(), std::io::Error> {
//     let mut file = File::create(format!("frame{}.ppm", index))?;
//     file.write_all(format!("P6\n{} {}\n255\n", frame.width(), frame.height()).as_bytes())?;
//     file.write_all(frame.data(0))?;
//     Ok(())
// }

// fn dump_metadata(file_name: &str) {
//     match ffmpeg::format::input(file_name) {
//         Ok(context) => {
//             for (k, v) in context.metadata().iter() {
//                 println!("{}: {}", k, v);
//             }

//             if let Some(stream) = context.streams().best(ffmpeg::media::Type::Video) {
//                 println!("Best video stream index: {}", stream.index());
//             }

//             if let Some(stream) = context.streams().best(ffmpeg::media::Type::Audio) {
//                 println!("Best audio stream index: {}", stream.index());
//             }

//             if let Some(stream) = context.streams().best(ffmpeg::media::Type::Subtitle) {
//                 println!("Best subtitle stream index: {}", stream.index());
//             }

//             println!(
//                 "duration (seconds): {:.2}",
//                 context.duration() as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE)
//             );

//             for stream in context.streams() {
//                 println!("stream index {}:", stream.index());
//                 println!("\ttime_base: {}", stream.time_base());
//                 println!("\tstart_time: {}", stream.start_time());
//                 println!("\tduration (stream timebase): {}", stream.duration());
//                 println!(
//                     "\tduration (seconds): {:.2}",
//                     stream.duration() as f64 * f64::from(stream.time_base())
//                 );
//                 println!("\tframes: {}", stream.frames());
//                 println!("\tdisposition: {:?}", stream.disposition());
//                 println!("\tdiscard: {:?}", stream.discard());
//                 println!("\trate: {}", stream.rate());

//                 let codec =
//                     ffmpeg::codec::context::Context::from_parameters(stream.parameters()).unwrap();
//                 println!("\tmedium: {:?}", codec.medium());
//                 println!("\tid: {:?}", codec.id());

//                 if codec.medium() == ffmpeg::media::Type::Video {
//                     if let Ok(video) = codec.decoder().video() {
//                         println!("\tbit_rate: {}", video.bit_rate());
//                         println!("\tmax_rate: {}", video.max_bit_rate());
//                         println!("\tframe_rate: {:?}", video.frame_rate());
//                         println!("\tdelay: {}", video.delay());
//                         println!("\tvideo.width: {}", video.width());
//                         println!("\tvideo.height: {}", video.height());
//                         println!("\tvideo.format: {:?}", video.format());
//                         println!("\tvideo.has_b_frames: {}", video.has_b_frames());
//                         println!("\tvideo.aspect_ratio: {}", video.aspect_ratio());
//                         println!("\tvideo.color_space: {:?}", video.color_space());
//                         println!("\tvideo.color_range: {:?}", video.color_range());
//                         println!("\tvideo.color_primaries: {:?}", video.color_primaries());
//                         println!(
//                             "\tvideo.color_transfer_characteristic: {:?}",
//                             video.color_transfer_characteristic()
//                         );
//                         println!("\tvideo.chroma_location: {:?}", video.chroma_location());
//                         println!("\tvideo.references: {}", video.references());
//                         println!("\tvideo.intra_dc_precision: {}", video.intra_dc_precision());
//                     }
//                 } else if codec.medium() == ffmpeg::media::Type::Audio {
//                     if let Ok(audio) = codec.decoder().audio() {
//                         println!("\tbit_rate: {}", audio.bit_rate());
//                         println!("\tmax_rate: {}", audio.max_bit_rate());
//                         println!("\tdelay: {}", audio.delay());
//                         println!("\taudio.rate: {}", audio.rate());
//                         println!("\taudio.channels: {}", audio.channels());
//                         println!("\taudio.format: {:?}", audio.format());
//                         println!("\taudio.frames: {}", audio.frames());
//                         println!("\taudio.align: {}", audio.align());
//                         println!("\taudio.channel_layout: {:?}", audio.channel_layout());
//                     }
//                 }
//             }

//             // Dump frames!
//             // ffmpeg_test::dump_frames(file_name);
//             //panic!();
//         }

//         Err(error) => println!("error: {}", error),
//     };
// }
