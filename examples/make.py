import os, glob, subprocess, time
script_path = os.path.dirname(os.path.realpath(__file__));
for c_file in glob.glob(script_path + "/*/*.c", recursive=False):
  print("compiling", c_file)
  args = [
    "gcc",          # compiler
    "-std=c11"      # C11 mode
    "-L",           # lib path flag
    "./",           # where to look for libs
    "-lreowolf_rs", # add lib called "reowolf_rs"
    "-Wl,-R./",     # pass -R flag to linker: produce relocatable object
    c_file,         # input source file
    "-o",           # output flag
    c_file[:-2]     # output filename
  ];
  subprocess.run(args)
input("Blocking until newline...");
