// Copyright 2016-2018 the Tectonic Project
// Licensed under the MIT License.

use std::collections::HashSet;
use std::path::Path;
use std::time;

use tectonic::engines::tex::TexResult;
use tectonic::engines::NoopIoEventBackend;
use tectonic::errors::{DefinitelySame, ErrorKind, Result};
use tectonic::io::testing::SingleInputFileIo;
use tectonic::io::{FilesystemIo, FilesystemPrimaryInputIo, IoProvider, IoStack, MemoryIo};
use tectonic::unstable_opts::UnstableOptions;
use tectonic::{TexEngine, XdvipdfmxEngine};
use tectonic_status_base::NoopStatusBackend;

#[path = "util/mod.rs"]
mod util;
use crate::util::{ensure_plain_format, test_path, ExpectedInfo};

struct TestCase {
    stem: String,
    expected_result: Result<TexResult>,
    check_synctex: bool,
    check_pdf: bool,
    extra_io: Vec<Box<dyn IoProvider>>,
    unstables: UnstableOptions,
}

impl TestCase {
    fn new(stem: &str) -> Self {
        TestCase {
            stem: stem.to_owned(),
            expected_result: Ok(TexResult::Spotless),
            check_synctex: false,
            check_pdf: false,
            extra_io: Vec::new(),
            unstables: UnstableOptions::default(),
        }
    }

    fn check_synctex(&mut self, check_synctex: bool) -> &mut Self {
        self.check_synctex = check_synctex;
        self
    }

    fn check_pdf(&mut self, check_pdf: bool) -> &mut Self {
        self.check_pdf = check_pdf;
        self
    }

    fn with_fs(&mut self, path: &Path) -> &mut Self {
        self.extra_io.push(Box::new(FilesystemIo::new(
            path,
            false,
            false,
            HashSet::new(),
        )));
        self
    }

    fn with_unstables(&mut self, unstables: UnstableOptions) -> &mut Self {
        self.unstables = unstables;
        self
    }

    fn expect(&mut self, result: Result<TexResult>) -> &mut Self {
        self.expected_result = result;
        self
    }

    fn expect_msg(&mut self, msg: &str) -> &mut Self {
        self.expect(Err(ErrorKind::Msg(msg.to_owned()).into()))
    }

    fn go(&mut self) {
        util::set_test_root();

        let expect_xdv = self.expected_result.is_ok();

        let mut p = test_path(&[]);

        // IoProvider for the format file; with magic to generate the format
        // on-the-fly if needed.
        let mut fmt =
            SingleInputFileIo::new(&ensure_plain_format().expect("couldn't write format file"));

        // Set up some useful paths, and the IoProvider for the primary input file.
        p.push("tex-outputs");
        p.push(&self.stem);
        p.set_extension("tex");
        let texname = p.file_name().unwrap().to_str().unwrap().to_owned();
        let mut tex = FilesystemPrimaryInputIo::new(&p);

        p.set_extension("xdv");
        let xdvname = p.file_name().unwrap().to_str().unwrap().to_owned();

        p.set_extension("pdf");
        let pdfname = p.file_name().unwrap().to_str().unwrap().to_owned();

        // MemoryIo layer that will accept the outputs.
        let mut mem = MemoryIo::new(true);

        // We only need the assets when running xdvipdfmx, but due to how
        // ownership works with IoStacks, it's easier to just unconditionally
        // add this layer.
        let mut assets = FilesystemIo::new(&test_path(&["assets"]), false, false, HashSet::new());

        let expected_log = ExpectedInfo::read_with_extension(&mut p, "log");

        // Run the engine(s)!
        let res = {
            let mut io_list: Vec<&mut dyn IoProvider> =
                vec![&mut mem, &mut tex, &mut fmt, &mut assets];
            for io in &mut self.extra_io {
                io_list.push(&mut **io);
            }
            let mut io = IoStack::new(io_list);

            let mut events = NoopIoEventBackend::new();
            let mut status = NoopStatusBackend::default();

            let tex_res = TexEngine::new().process(
                &mut io,
                &mut events,
                &mut status,
                "plain.fmt",
                &texname,
                &self.unstables,
            );

            if self.check_pdf && tex_res.definitely_same(&Ok(TexResult::Spotless)) {
                XdvipdfmxEngine::new()
                    .with_compression(false)
                    .with_deterministic_tags(true)
                    .with_date(
                        time::SystemTime::UNIX_EPOCH
                            .checked_add(time::Duration::from_secs(1_456_304_492))
                            .unwrap(),
                    )
                    .process(
                        &mut io,
                        &mut events,
                        &mut status,
                        &xdvname,
                        &pdfname,
                        &self.unstables,
                    )
                    .unwrap();
            }

            tex_res
        };

        if !res.definitely_same(&self.expected_result) {
            panic!(format!(
                "expected TeX result {:?}, got {:?}",
                self.expected_result, res
            ));
        }

        // Check that outputs match expectations.

        let files = mem.files.borrow();

        expected_log.test_from_collection(&files);

        if expect_xdv {
            ExpectedInfo::read_with_extension(&mut p, "xdv").test_from_collection(&files);
        }

        if self.check_synctex {
            ExpectedInfo::read_with_extension_gz(&mut p, "synctex.gz").test_from_collection(&files);
        }

        if self.check_pdf {
            ExpectedInfo::read_with_extension(&mut p, "pdf").test_from_collection(&files);
        }
    }
}

// Keep these alphabetized.

#[test]
fn a4paper() {
    let unstables = UnstableOptions {
        paper_size: Some(String::from("a4")),
        ..Default::default()
    };
    TestCase::new("a4paper")
        .with_unstables(unstables)
        .check_pdf(true)
        .go()
}

#[test]
fn file_encoding() {
    // Need to do this here since we call test_path unusually early.
    util::set_test_root();

    TestCase::new("file_encoding.tex")
        .with_fs(&test_path(&["tex-outputs"]))
        .expect(Ok(TexResult::Warnings))
        .go()
}

/// An issue triggered by a bug in how the I/O subsystem reported file offsets
/// after an ungetc() call.
#[test]
fn issue393_ungetc() {
    TestCase::new("issue393_ungetc")
        .expect(Ok(TexResult::Warnings))
        .go()
}

#[test]
fn md5_of_hello() {
    TestCase::new("md5_of_hello").check_pdf(true).go()
}

#[test]
fn negative_roman_numeral() {
    TestCase::new("negative_roman_numeral").go()
}

#[test]
fn otf_basic() {
    TestCase::new("otf_basic")
        .expect(Ok(TexResult::Warnings))
        .go()
}

#[test]
fn prim_creationdate() {
    TestCase::new("prim_creationdate").go()
}

#[test]
fn prim_filedump() {
    TestCase::new("prim_filedump").go()
}

#[test]
fn prim_filemoddate() {
    // Git doesn't preserve mtimes, so manually force the mtime of the input
    // file to something repeatable.
    util::set_test_root();
    let path = test_path(&["tex-outputs", "prim_filemoddate.tex"]);
    let t = filetime::FileTime::from_unix_time(1_603_835_905, 0);
    filetime::set_file_mtime(path, t).expect("failed to set input file mtime");

    TestCase::new("prim_filemoddate").go()
}

#[test]
fn prim_filesize() {
    TestCase::new("prim_filesize").go()
}

#[test]
fn tex_logo() {
    TestCase::new("tex_logo").go()
}

#[test]
fn pdfoutput() {
    TestCase::new("pdfoutput").go()
}

#[test]
fn pdfimages() {
    TestCase::new("png_formats").check_pdf(true).go()
}

#[test]
fn redbox_png() {
    TestCase::new("redbox_png").check_pdf(true).go()
}

#[test]
fn synctex() {
    TestCase::new("synctex").check_synctex(true).go()
}

#[test]
fn unicode_file_name() {
    TestCase::new("hallöchen 🐨 welt 🌍.tex")
        .expect(Ok(TexResult::Warnings))
        .go()
}

#[test]
fn tectoniccodatokens_errinside() {
    TestCase::new("tectoniccodatokens_errinside")
        .expect_msg("halted on potentially-recoverable error as specified")
        .go()
}

#[test]
fn tectoniccodatokens_noend() {
    TestCase::new("tectoniccodatokens_noend")
        .expect_msg("*** (job aborted, no legal \\end found)")
        .go()
}

#[test]
fn tectoniccodatokens_ok() {
    TestCase::new("tectoniccodatokens_ok").go()
}

#[test]
fn the_letter_a() {
    TestCase::new("the_letter_a").check_pdf(true).go()
}
