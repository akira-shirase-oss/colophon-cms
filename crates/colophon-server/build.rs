//! Stamps the binary with its git commit and build date (`colophon_version::stamp`), for
//! `--version` and the server's `/version`.
fn main() {
    colophon_version::stamp::emit();
}
