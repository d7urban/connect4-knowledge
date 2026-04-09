fn main() {
    if let Err(err) = connect4_knowledge::run(std::env::args().skip(1)) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
