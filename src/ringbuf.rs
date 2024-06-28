use crate::page::{Page, ReadResult};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct Reader {
    path: PathBuf,
    write_page_count: Arc<RwLock<usize>>,
    read_page_no: usize,
    read_page: Page,
    read_start_byte: usize,
    max_total_pages: usize,
}

#[derive(Clone)]
pub struct Writer {
    path: PathBuf,
    write_page_count: Arc<RwLock<usize>>,
    write_page_no: usize,
    write_page: Page,
    max_total_pages: usize,
}

const TEMP_MAX_TOTAL_PAGES: usize = 4;
const PAGE_EXT: &str = "page.bin";

// TODO: fn find_pages() {}

pub fn new<P: Into<PathBuf>>(path: P) -> (Writer, Reader) {
    let path = path.into();
    let _ = std::fs::create_dir_all(&path);

    // this should initialize to the number of files
    let temp_latest_file_no = 0;
    let wp_count = Arc::new(RwLock::new(temp_latest_file_no));
    let page = Page::new(
        &path
            .join(temp_latest_file_no.to_string())
            .with_extension(PAGE_EXT)
            .to_str()
            .expect("this should always be unicode"),
    );

    (
        Writer {
            path: path.clone(),
            write_page_count: wp_count.clone(),
            write_page_no: 0,
            write_page: page.clone(),
            max_total_pages: TEMP_MAX_TOTAL_PAGES,
        },
        Reader {
            path,
            write_page_count: wp_count,
            read_page_no: 0,
            read_page: page,
            read_start_byte: 0,
            max_total_pages: TEMP_MAX_TOTAL_PAGES,
        },
    )
}

impl Writer {
    fn page_flip(&mut self) {
        let page_count = self
            .write_page_count
            .read()
            .expect("something went really bad with your lock");

        if self.write_page_no < *page_count {
            self.write_page_no += 1;
            return;
        }

        if self.write_page_no == *page_count {
            drop(page_count);

            let mut page_count = self
                .write_page_count
                .write()
                .expect("something went really bad with your lock");

            if self.write_page_no < *page_count {
                self.write_page_no += 1;
                return;
            }

            *page_count += 1;
            self.write_page_no += 1;

            if *page_count >= self.max_total_pages {
                std::fs::remove_file(
                    &self
                        .path
                        .join((*page_count - self.max_total_pages).to_string())
                        .with_extension(PAGE_EXT)
                        .to_str()
                        .expect("this should always be unicode"),
                )
                .expect("something went wrong deleting an old file");
            }
        }
    }

    pub fn push<T: AsRef<[u8]>>(&mut self, input: T) {
        loop {
            let _ = match self.write_page.try_push(&input) {
                Ok(0) => 0, // PAGE FULL / Continue
                Ok(_) => break,
                Err(e) => panic!("{:#?}", e),
            };

            self.page_flip();

            self.write_page = Page::new(
                &self
                    .path
                    .join(self.write_page_no.to_string())
                    .with_extension(PAGE_EXT)
                    .to_str()
                    .expect("this should always be unicode"),
            );
        }
    }
}

impl Reader {
    pub fn pop(&mut self) -> Option<String> {
        loop {
            match self.read_page.try_pop(self.read_start_byte) {
                Ok(None) => return None, // no new messages
                Ok(Some(ReadResult::Continue)) => {}
                Ok(Some(ReadResult::Msg(m))) => {
                    self.read_start_byte += m.len() + 1;
                    return Some(m.to_string());
                }
                Err(e) => panic!("{e}"),
            };

            let page_count = self
                .write_page_count
                .read()
                .expect("something went really wrong with your lock");

            self.read_page_no = std::cmp::max(
                self.read_page_no + 1,
                page_count.saturating_sub(self.max_total_pages),
            );

            self.read_start_byte = 0;
            self.read_page = Page::new(
                &self
                    .path
                    .join(self.read_page_no.to_string())
                    .with_extension(PAGE_EXT)
                    .to_str()
                    .expect("this should always be unicode"),
            );
        }
    }
}

#[test]
fn seq_test() {
    let (mut tx, mut rx) = new("test-seq");

    let now = std::time::Instant::now();
    for i in 0..50_000_000 {
        tx.push(i.to_string());
    }

    for i in 0..50_000_000 {
        let m = rx.pop().unwrap();
        assert_eq!(m, i.to_string());
    }

    eprintln!("took {} ms", now.elapsed().as_millis());
}

#[test]
fn spsc_test() {
    let (mut tx, mut rx) = new("test-spsc");

    tx.push("a");
    assert_eq!(rx.pop().unwrap(), "a".to_string());
    assert!(rx.pop().is_none());

    let now = std::time::Instant::now();
    let t = std::thread::spawn(move || {
        for i in 0..50_000_000 {
            tx.push(i.to_string());
        }
    });

    // let _ = t.join().unwrap();

    let mut i = 0;
    loop {
        if i == 50_000_000 {
            break;
        }

        let m = match rx.pop() {
            Some(m) => m,
            None => continue,
        };

        eprintln!("{m}");

        assert_eq!(m, i.to_string());
        i += 1;
    }

    // let _ = t.join().unwrap();

    eprintln!("took {} ms", now.elapsed().as_millis());
}
// deleting pages on pop makes life much easier as opposed to deleting
// old pages on push which might screw things up
