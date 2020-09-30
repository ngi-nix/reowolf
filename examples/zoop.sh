#!/bin/bash
for options in {1..5}
do
	for forwards in {0..15}
	do
		./bench_11/main.exe $forwards $options 0
	done
done