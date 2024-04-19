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
