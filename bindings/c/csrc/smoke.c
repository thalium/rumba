/*
 * End-to-end smoke test for the C bindings: compiles against the cbindgen
 * header and links the cdylib, so it catches header/ABI drift that the
 * in-crate Rust tests cannot see.
 *
 * Exits 0 on success, non-zero on the first failed check.
 */
#include "rumba.h"
#include <stdint.h>
#include <stdio.h>

int main(void) {
    /* Build `v0 + 3` from the constructors and evaluate it at v0 = 10. */
    void *x = rumba_make_var(0);
    void *three = rumba_make_const(3);
    void *sum = rumba_expr_add(x, three); /* consumes x and three */

    uint64_t vars[] = {10};
    uint64_t got = rumba_expr_eval(sum, 64, vars, 1);
    if (got != 13) {
        fprintf(stderr, "eval(v0 + 3) at v0=10: expected 13, got %llu\n",
                (unsigned long long)got);
        return 1;
    }

    rumba_expr_free(sum);

#if defined(DEFINE_RUMBA_PARSE)
    /* Parse an MBA, simplify it, and check it evaluates like v0 + v1. */
    void *parsed = NULL;
    if (rumba_expr_parse("(v0^v1)+2*(v0&v1)", &parsed) != 0 || parsed == NULL) {
        fprintf(stderr, "failed to parse MBA: %s\n", get_err_str());
        return 2;
    }

    void *simplified = rumba_expr_simplify(parsed, 64); /* consumes parsed */
    if (simplified == NULL) {
        fprintf(stderr, "failed to simplify MBA: %s\n", get_err_str());
        return 3;
    }

    uint64_t vars2[] = {123, 456};
    uint64_t got2 = rumba_expr_eval(simplified, 64, vars2, 2);
    if (got2 != 123 + 456) {
        fprintf(stderr, "eval(simplified) at (123, 456): expected 579, got %llu\n",
                (unsigned long long)got2);
        return 4;
    }

    rumba_expr_free(simplified);
#endif

    printf("C smoke test passed\n");
    return 0;
}
