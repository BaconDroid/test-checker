use std::hint::black_box;
use std::time::Instant;

use test_checker::api::check_hook_input;
use test_checker::detector::TestDetector;
use test_checker::filters::apply_filters;
use test_checker::models::{Confidence, Finding};
use test_checker::output::{build_findings_xml, format_hook_message};

const WARMUP_ITERATIONS: u32 = 100;
const MEASURED_ITERATIONS: u32 = 1_000;
const SAMPLES: usize = 5;

const PYTHON_CLEAN: &str = "def test_adds():\n    assert add(1, 2) == 3\n";
const PYTHON_WEAKENED: &str =
    "def test_adds():\n    value = add(1, 2)\n    @pytest.mark.skip\n    assert value == value\n";
const CPP_CLEAN: &str = "TEST(Math, Adds) {\n  EXPECT_EQ(add(1, 2), 3);\n}\n";
const BASH_CLEAN: &str = "@test \"adds\" {\n  run add 1 2\n  assert_success\n}\n";

fn measure(name: &str, run: &mut impl FnMut()) {
    for _ in 0..WARMUP_ITERATIONS {
        run();
    }
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let start = Instant::now();
        for _ in 0..MEASURED_ITERATIONS {
            run();
        }
        samples.push(start.elapsed() / MEASURED_ITERATIONS);
    }
    samples.sort_unstable();
    let best = samples.first().copied().unwrap_or_default();
    let median = samples.get(samples.len() / 2).copied().unwrap_or_default();
    println!("{name}: best {best:?}/iter, median {median:?}/iter");
}

fn main() {
    let detector = TestDetector::new();
    measure("detect_python_clean", &mut || {
        black_box(detector.detect(
            black_box(PYTHON_CLEAN),
            black_box("tests/test_app.py"),
            true,
            true,
        ));
    });
    measure("detect_python_weakened", &mut || {
        black_box(detector.detect(
            black_box(PYTHON_WEAKENED),
            black_box("tests/test_app.py"),
            true,
            true,
        ));
    });
    measure("detect_unsupported_extension", &mut || {
        black_box(detector.detect(
            black_box(PYTHON_CLEAN),
            black_box("config.toml"),
            true,
            true,
        ));
    });
    measure("detect_cpp_clean", &mut || {
        black_box(detector.detect(
            black_box(CPP_CLEAN),
            black_box("tests/math_test.cpp"),
            true,
            true,
        ));
    });
    measure("detect_bash_clean", &mut || {
        black_box(detector.detect(
            black_box(BASH_CLEAN),
            black_box("tests/math.bats"),
            true,
            true,
        ));
    });
    measure("detect_edit_weakened", &mut || {
        black_box(detector.detect_edit(
            black_box("def test_adds():\n    assert add(1, 2) == 3\n"),
            black_box("def test_adds():\n    assert add(1, 2) == add(1, 2)\n"),
            black_box("tests/test_app.py"),
            true,
            true,
        ));
    });

    let findings: Vec<Finding> = (0..200)
        .map(|index| {
            Finding::new(
                "skip-added",
                Confidence::High,
                format!("finding {index}"),
                index + 1,
                "tests/test_app.py",
                "    @pytest.mark.skip",
            )
        })
        .collect();
    measure("apply_filters_many", &mut || {
        black_box(apply_filters(black_box(&findings)));
    });
    measure("build_findings_xml_many", &mut || {
        black_box(build_findings_xml(
            black_box(&findings),
            black_box("tests/test_app.py"),
        ));
    });
    measure("format_hook_message_many", &mut || {
        black_box(format_hook_message(black_box(&findings), black_box("")));
    });

    let write_input = r#"{"tool_name":"Write","tool_input":{"file_path":"tests/test_app.py","content":"def test_adds():\n    assert add(1, 2) == add(1, 2)\n"}}"#;
    let edit_input = r#"{"tool_name":"Edit","tool_input":{"file_path":"tests/test_app.py","old_string":"assert add(1, 2) == 3","new_string":"assert add(1, 2) == add(1, 2)"}}"#;
    measure("check_hook_input_write_weakened", &mut || {
        black_box(check_hook_input(
            black_box(write_input),
            black_box("{{findings}}"),
        ));
    });
    measure("check_hook_input_edit_weakened", &mut || {
        black_box(check_hook_input(
            black_box(edit_input),
            black_box("{{findings}}"),
        ));
    });
}
