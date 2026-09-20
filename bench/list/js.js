function build(n) { let acc = null; for (; n !== 0; n--) acc = { head: n, tail: acc }; return acc; }
function sum(xs) { let acc = 0; for (; xs !== null; xs = xs.tail) acc += xs.head; return acc; }
let total = 0;
for (let r = 20; r !== 0; r--) total += sum(build(1000000 + r));
console.log(total);
