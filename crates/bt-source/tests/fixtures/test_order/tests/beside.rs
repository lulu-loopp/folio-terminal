// An integration test of the test-order fixture: cargo builds the bin it names before it runs.

#[test]
fn a_bin_located_where_cargo_built_it() {
    let _ = std::path::Path::new(env!("CARGO_BIN_EXE_folio"));
}
