#!/bin/bash
for included in {0..13}
do
	./bench_13/main.exe 65535 $included 13
done