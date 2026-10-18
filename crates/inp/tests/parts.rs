//! `*PART`, `*ASSEMBLY` and `*INSTANCE` are not supported. The model must be flat.

use fh_inp::parse;

#[test]
fn part_assembly_and_instance_fail_with_a_clear_error() {
    for keyword in [
        "PART, NAME=A",
        "END PART",
        "ASSEMBLY, NAME=A",
        "END ASSEMBLY",
        "INSTANCE, NAME=I, PART=A",
        "END INSTANCE",
    ] {
        let name = keyword.split(',').next().unwrap();
        assert_eq!(
            parse(&format!("*{keyword}\n")).unwrap_err().to_string(),
            format!("line 1, *{name}: parts and assemblies are not supported; write a flat model (INP002)")
        );
    }
}
