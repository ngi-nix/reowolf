import os, glob, subprocess
script_path = os.path.dirname(os.path.realpath(__file__));
for c_file in glob.glob(script_path + "/*/*.c", recursive=False):
	print("compiling", c_file)
	args = [
		"gcc", 
		"-L",
		"../target/release",
		"-lreowolf_rs",
		"-Wl,-R../target/release",
		c_file,
		"-o",
		c_file[:-2]
	];
	subprocess.run(args);