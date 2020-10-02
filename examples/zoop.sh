#!/bin/bash
for ports in 2 4 6 8 10 12 14 16
do
	./bench_19/main.exe 98 7500 7000 16 $ports y y 192 168 1 4 1000
done
