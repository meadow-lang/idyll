package main

import "fmt"

type node struct {
	head int64
	tail *node
}

func build(n int64) *node {
	var acc *node
	for ; n != 0; n-- {
		acc = &node{n, acc}
	}
	return acc
}

func mapRev(f func(int64) int64, xs *node) *node {
	var acc *node
	for ; xs != nil; xs = xs.tail {
		acc = &node{f(xs.head), acc}
	}
	return acc
}

func sum(xs *node) int64 {
	var acc int64
	for ; xs != nil; xs = xs.tail {
		acc += xs.head
	}
	return acc
}

func main() {
	var total int64
	for r := int64(20); r != 0; r-- {
		k := r
		total += sum(mapRev(func(x int64) int64 { return x + k }, build(1000000)))
	}
	fmt.Println(total)
}
