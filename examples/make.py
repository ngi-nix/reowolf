import os, glob, subprocess, time, sys
script_path = os.path.dirname(os.path.realpath(__file__));
for c_file in glob.glob(script_path + "/*/*.c", recursive=False):
  if sys.platform != "linux" and sys.platform != "linux2" and "interop" in c_file:
    print("Not Linux! skipping", c_file)
    continue
  print("compiling", c_file)
  args = [
    "gcc",          # compiler
    "-std=c11",     # C11 mode
    "-Wl,-R./",     # pass -R flag to linker: produce relocatable object
    c_file,         # input source file
    "-o",           # output flag
    c_file[:-2],    # output filename
    "-L",           # lib path flag
    "./",           # where to look for libs
    "-lreowolf_rs"  # add lib called "reowolf_rs"
  ];
  subprocess.run(args)
input("Blocking until newline...");
