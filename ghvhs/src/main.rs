use ghvhs::frame::Frame;

fn main() {
    println!("Starting GHVHS ⏪");

    let mut frame_counter = 0;
    loop {

        std::thread::sleep(std::time::Duration::from_secs(3600));
        frame_counter += 1;
    } 
}
