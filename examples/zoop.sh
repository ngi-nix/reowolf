#!/bin/bash
for syncs in {0..16}
do
	./bench_8/main.exe $syncs
done