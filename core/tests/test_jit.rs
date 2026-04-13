#![cfg(all(feature = "parse", feature = "jit"))]

#[cfg(test)]
mod jit {
    use rand::random_range;
    use rumba_core::{jit, parser::parse_expr, varint::make_mask};

    #[test]
    fn test_jit() {
        let functions = [
            "v0 + v1 + 2",
            "-1*~v1+2*(v0^v1)-3*~(v0|~v1)-2*(v0&~v1)-1*(v0&v1)",
            "-1*~(v0&~v1)+7*v1+2*~v0+8*(v0&~v1)-6*(v0&v1)",
            "1*~(v0&v1)-6*v1+5*~(v0|v1)+6*~(v0|~v1)+6*(v0&~v1)+13*(v0&v1)",
            "2*v0+1*~(v0&~v0)-1*(v0|~v1)+3*~(v0|v1)-2*(v0&~v1)-2*(v0&v1)",
            "-7*~(v0^v1)+2*v0+2*~(v0|v1)-5*~(v0|~v1)-7*(v0&~v1)",
        ];

        for s in functions {
            let e = parse_expr(s).unwrap();
            let jit_fn = jit::compile(&e);

            let t = 2;
            let mask = make_mask(64);

            for _ in 0..1000 {
                let vars: Vec<_> = (0..=t).map(|_| random_range(0..=mask)).collect();

                let v1 = e.eval(&vars).get(mask);
                let v2 = jit_fn.eval(&vars) & mask;

                assert_eq!(v1, v2);
            }
        }
    }
}
