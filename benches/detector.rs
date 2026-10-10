use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use test_checker::api::check_hook_input;
use test_checker::detector::{Change, TestDetector};

const PYTHON_TEST: &str =
    "def test_add():\n    assert add(1, 2) == 3\n\ndef test_weakened():\n    assert True\n";

const TYPESCRIPT_TEST: &str = "it('adds', () => {\n  expect(add(1, 2)).toBe(3);\n});\n";

fn change(file_path: &str, language: &'static str, old_text: &str, new_text: &str) -> Change {
    Change {
        file_path: file_path.to_owned(),
        language,
        new_text: new_text.to_owned(),
        old_text: old_text.to_owned(),
        is_new: false,
        is_test_file: true,
    }
}

fn benchmark_detector(criterion: &mut Criterion) {
    let detector = TestDetector::new();
    criterion.bench_function("detect_python", |bencher| {
        bencher.iter(|| {
            black_box(detector.detect(&change(
                "test_sample.py",
                "python",
                "",
                black_box(PYTHON_TEST),
            )))
        });
    });
    criterion.bench_function("detect_typescript", |bencher| {
        bencher.iter(|| {
            black_box(detector.detect(&change(
                "sample.test.ts",
                "ts",
                "",
                black_box(TYPESCRIPT_TEST),
            )))
        });
    });
    criterion.bench_function("detect_unsupported_extension", |bencher| {
        bencher.iter(|| {
            black_box(detector.detect(&change(
                "sample.unknown",
                "ts",
                "",
                black_box(TYPESCRIPT_TEST),
            )))
        });
    });
}

fn benchmark_hook_api(criterion: &mut Criterion) {
    let edit_input = r#"{"tool_name":"Edit","tool_input":{"file_path":"test_sample.py","old_string":"    assert add(1, 2) == 3","new_string":"    assert True"}}"#;
    let write_input = r#"{"tool_name":"Write","tool_input":{"file_path":"test_sample.py","content":"def test_add():\n    assert True\n"}}"#;

    criterion.bench_function("check_hook_input_edit_weakened", |bencher| {
        bencher.iter(|| {
            black_box(check_hook_input(
                black_box(edit_input),
                black_box("{{findings}}"),
            ))
        });
    });
    criterion.bench_function("check_hook_input_write_weakened", |bencher| {
        bencher.iter(|| {
            black_box(check_hook_input(
                black_box(write_input),
                black_box("{{findings}}"),
            ))
        });
    });
}

criterion_group!(benches, benchmark_detector, benchmark_hook_api);
criterion_main!(benches);
