fn main() {
    for a in std::env::args().skip(1) {
        let i: u16 = a.parse().unwrap();
        println!("{i} {}", fastfile_h1::code_source::code_const_name(i));
    }
}
