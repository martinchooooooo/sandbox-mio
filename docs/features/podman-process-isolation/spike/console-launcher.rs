// Console-subsystem launcher, standing in for build.exe: spawns the GUI child with inherited stdio.
use std::process::Command;

fn main() {
    let mut a = std::env::args().skip(1);
    let child = a.next().unwrap();
    let log = a.next().unwrap();
    let st = Command::new(child).arg(&log).status().unwrap();
    println!("gui child exited: {st}");
    print!("{}", std::fs::read_to_string(&log).unwrap());
}
