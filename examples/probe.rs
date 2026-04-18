fn main() {
    let src = "progressive FI_Training:\n  steps v = [2, 4, 8, 12, 18]\n  sessions >= 3\n  stable(visual)\n  FI {v}s LH3s";
    let tokens = contingency_dsl::tokenize(src).unwrap();
    for t in &tokens {
        println!("{:?} {:?}", t.ty, t.value);
    }
}
