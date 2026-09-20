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

func sum(xs *node) int64 {
	var acc int64
	for ; xs != nil; xs = xs.tail {
		acc += xs.head
	}
	return acc
}

func main() {
	var total int64
	for r := 20; r != 0; r-- {
		total += sum(build(int64(1000000 + r)))
	}
	fmt.Println(total)
}
