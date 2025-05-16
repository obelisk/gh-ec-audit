use ghvhs::frame::Frame;

fn main() {
    println!("Starting GHVHS - Rewind The Tape <<");

    let configuration_path = std::env::args().nth(1);

    let configuration = match ghvhs::config::get_configuration(configuration_path) {
        Ok(config) => config,
        Err(e) => {
            println!("{}", e);
            return;
        }
    };

    let mut frame_counter = 0;
    loop {
        println!("Frame {}: Generating frame...", frame_counter);

        let frame = match Frame::generate(&configuration) {
            Ok(frame) => frame,
            Err(e) => {
                println!("Error generating frame: {}", e);
                return;
            }
        };

        println!("{}", serde_json::to_string_pretty(&frame).unwrap());

        std::thread::sleep(std::time::Duration::from_secs(3600));
        frame_counter += 1;
    }
}
