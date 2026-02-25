use std::ptr;

pub type Ptr = *mut Node;

#[repr(C)]
pub enum Node {
    App(Ptr, Ptr),

    S,
    K,
    I,
    B,
    C,
    Y,

    P,
    O,

    Pair(i64, i64),
    Int(i64),

    Forward(Ptr),
}

pub struct Heap {
    from_space: Vec<Node>,
    to_space: Vec<Node>,
    alloc_ptr: usize,
    scan_ptr: usize,
    size: usize,
}

impl Heap {
    pub fn new(size: usize) -> Self {
        Heap {
            from_space: Vec::with_capacity(size),
            to_space: Vec::with_capacity(size),
            alloc_ptr: 0,
            scan_ptr: 0,
            size,
        }
    }

    unsafe fn alloc(&mut self, node: Node, roots: &mut [Ptr]) -> Ptr {
        if self.from_space.len() >= self.size {
            self.collect(roots);
        }

        self.from_space.push(node);
        self.from_space.as_mut_ptr().add(self.from_space.len() - 1)
    }

    unsafe fn collect(&mut self, roots: &mut [Ptr]) {
        self.to_space.clear();
        self.scan_ptr = 0;

        for root in roots.iter_mut() {
            *root = self.copy(*root);
        }

        while self.scan_ptr < self.to_space.len() {
            let node_ptr = self.to_space.as_mut_ptr().add(self.scan_ptr);
            self.scan_ptr += 1;

            match &mut *node_ptr {
                Node::App(f, x) => {
                    *f = self.copy(*f);
                    *x = self.copy(*x);
                }
                _ => {}
            }
        }

        std::mem::swap(&mut self.from_space, &mut self.to_space);
    }

    unsafe fn copy(&mut self, ptr: Ptr) -> Ptr {
        if ptr.is_null() {
            return ptr;
        }

        match &mut *ptr {
            Node::Forward(new_ptr) => return *new_ptr,
            _ => {}
        }

        let new_location = self.to_space.len();
        self.to_space.push(ptr::read(ptr));

        let new_ptr = self.to_space.as_mut_ptr().add(new_location);

        *ptr = Node::Forward(new_ptr);

        new_ptr
    }
}

pub unsafe fn reduce(heap: &mut Heap, mut current: Ptr) -> Ptr {
    let mut stack: Vec<Ptr> = vec![];

    loop {
        match &*current {
            Node::App(f, x) => {
                stack.push(*x);
                current = *f;
            }

            Node::I => {
                return stack.pop().unwrap();
            }

            Node::K => {
                let x = stack.pop().unwrap();
                stack.pop();
                return x;
            }

            Node::S => {
                let x = stack.pop().unwrap();
                let g = stack.pop().unwrap();
                let f = stack.pop().unwrap();

                let fx = heap.alloc(Node::App(f, x), &mut stack);
                let gx = heap.alloc(Node::App(g, x), &mut stack);
                return heap.alloc(Node::App(fx, gx), &mut stack);
            }

            Node::P => {
                let b = reduce(heap, stack.pop().unwrap());
                let a = reduce(heap, stack.pop().unwrap());

                let av = match &*a {
                    Node::Int(v) => *v,
                    _ => panic!("Expected Int"),
                };

                let bv = match &*b {
                    Node::Int(v) => *v,
                    _ => panic!("Expected Int"),
                };

                return heap.alloc(Node::Pair(av, bv), &mut stack);
            }

            Node::O => {
                let pair = reduce(heap, stack.pop().unwrap());

                match &*pair {
                    Node::Pair(a, _) => {
                        return heap.alloc(Node::Int(*a), &mut stack);
                    }
                    _ => panic!("Expected Pair"),
                }
            }

            Node::Pair(_, _) | Node::Int(_) => {
                return current;
            }

            _ => panic!("Unimplemented combinator"),
        }
    }
}
