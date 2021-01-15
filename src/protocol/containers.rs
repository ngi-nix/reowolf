/// Containers.rs
///
/// Contains specialized containers for the parser/compiler
/// TODO: Actually implement, I really want to remove all of the identifier
///     allocations.

use std::collections::LinkedList;

const PAGE_SIZE: usize = 4096;

struct StringPage {
    buffer: [u8; PAGE_SIZE],
    remaining: usize,
    next_page: Option<Box<StringPage>>,
}

impl StringPage {
    fn new() -> Self{
        Self{
            buffer: [0; PAGE_SIZE],
            remaining: PAGE_SIZE,
            next_page: None
        }
    }
}

/// Custom allocator for strings that are copied and remain valid during the
/// complete compilation phase. May perform multiple allocations but the
/// previously allocated strings remain valid. Because we will usually allocate
/// quite a number of these we will allocate a buffer upon construction of the
/// StringAllocator.
pub(crate) struct StringAllocator {
    first_page: Box<StringPage>,
    last_page: *mut StringPage,
}

unsafe impl Send for StringAllocator {}

impl StringAllocator {
    pub(crate) fn new() -> StringAllocator {
        let mut page = Box::new(StringPage::new());
        let page_ptr = unsafe { page.as_mut() as *mut StringPage };
        StringAllocator{
            first_page: page,
            last_page: page_ptr,
        }
    }

    pub(crate) fn alloc(&mut self, data: &[u8]) -> Result<&'static str, String> {
        let data_len = data.len();
        if data_len > PAGE_SIZE {
            return Err(format!(
                "string is too large ({} bytes exceeds the maximum of {})",
                data_len, PAGE_SIZE
            ));
        }

        // Because we're doing a copy anyway, we might as well perform the
        // UTF-8 checking now. Such that it is safe to do an unchecked
        // `from_utf8_unchecked` later.
        let data = std::str::from_utf8(data);
        if let Err(_) = data {
            return Err(format!("invalid utf8-string"));
        }
        let data = data.unwrap();

        unsafe {
            if data_len > (*self.last_page).remaining {
                // Allocate new page
                let mut new_page = Box::new(StringPage::new());
                let new_page_ptr = new_page.as_mut() as *mut StringPage;
                (*self.last_page).next_page = Some(new_page);
                self.last_page = new_page_ptr;
            }

            let remaining = (*self.last_page).remaining;
            debug_assert!(data_len <= remaining);
            let start = PAGE_SIZE - remaining;
            (*self.last_page).buffer[start..start+data_len].copy_from_slice(data.as_bytes());
            (*self.last_page).remaining -= data_len;

            Ok(std::str::from_utf8_unchecked(&(*self.last_page).buffer[start..start+data_len]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc() {
        // Make sure pointers are somewhat correct
        let mut alloc = StringAllocator::new();
        assert!(alloc.first_page.next_page.is_none());
        assert_eq!(alloc.first_page.as_ref() as *const StringPage, alloc.last_page);

        // Insert and make page full, should not allocate another page yet
        let input = "I am a simple static string";
        let filler = " ".repeat(PAGE_SIZE - input.len());
        let ref_first = alloc.alloc(input.as_bytes()).expect("alloc first");
        let ref_filler = alloc.alloc(filler.as_bytes()).expect("alloc filler");
        assert!(alloc.first_page.next_page.is_none());
        assert_eq!(alloc.first_page.as_ref() as *const StringPage, alloc.last_page);

        let ref_second = alloc.alloc(input.as_bytes()).expect("alloc second");

        assert!(alloc.first_page.next_page.is_some());
        assert!(alloc.first_page.next_page.as_ref().unwrap().next_page.is_none());
        let last_page_ptr = alloc.first_page.next_page.as_ref().unwrap().as_ref() as *const StringPage;
        assert_eq!(last_page_ptr, alloc.last_page);

        assert_eq!(ref_first, input);
        assert_eq!(ref_filler, filler);
        assert_eq!(ref_second, input);
    }
}