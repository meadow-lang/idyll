use std::ptr;

pub type Ptr = *mut Node;

#[repr(C)]
pub enum Node {
    App(Ptr, Ptr),

    S,
    K,
    I,
    Y,
    B,
    C,
    W,

    Pair(Ptr, Ptr),
    Int(i64),
    Array(Ptr, usize),
    Indirect(Ptr),
    Data {
        tag: usize,
        fields: Ptr,
        arity: usize,
    },

    Add,
    Sub,
    Mul,
    Div,
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
            Node::Indirect(new_ptr) => return *new_ptr,
            _ => {}
        }

        let new_location = self.to_space.len();
        self.to_space.push(ptr::read(ptr));

        let new_ptr = self.to_space.as_mut_ptr().add(new_location);

        *ptr = Node::Indirect(new_ptr);

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
                // I x = x
                return stack.pop().unwrap();
            }

            Node::K => {
                // K x y = x
                let x = stack.pop().unwrap();
                stack.pop();
                return x;
            }

            Node::S => {
                // S f g x = f x (g x)
                let x = stack.pop().unwrap();
                let g = stack.pop().unwrap();
                let f = stack.pop().unwrap();

                let fx = heap.alloc(Node::App(f, x), &mut stack);
                let gx = heap.alloc(Node::App(g, x), &mut stack);
                return heap.alloc(Node::App(fx, gx), &mut stack);
            }

            Node::Y => {
                // Y f = f (Y f)
                let f = stack.pop().expect("Y requires 1 argument");
                let app_ptr = heap.alloc(Node::App(f, ptr::null_mut()), &mut stack);

                if let Node::App(_, ref mut x) = unsafe { &mut *app_ptr } {
                    *x = app_ptr;
                } else {
                    panic!("Expected App node for Y combinator");
                }

                return app_ptr;
            }

            Node::B => {
                // B f g x = f (g x)
                let f = stack.pop().unwrap();
                let g = stack.pop().unwrap();
                let x = stack.pop().unwrap();

                let gx = heap.alloc(Node::App(g, x), &mut stack);
                return heap.alloc(Node::App(f, gx), &mut stack);
            }

            Node::C => {
                // C f x y = f y x
                let f = stack.pop().unwrap();
                let x = stack.pop().unwrap();
                let y = stack.pop().unwrap();

                let fy = heap.alloc(Node::App(f, y), &mut stack);
                return heap.alloc(Node::App(fy, x), &mut stack);
            }

            Node::W => {
                // W f x = f x x
                let f = stack.pop().unwrap();
                let x = stack.pop().unwrap();

                let fx = heap.alloc(Node::App(f, x), &mut stack);
                return heap.alloc(Node::App(fx, x), &mut stack);
            }

            Node::Pair(_, _) | Node::Int(_) | Node::Array(_, _) => {
                return current;
            }

            _ => panic!("Unimplemented combinator"),
        }
    }
}
