"""Print the frequency of each mode from the job.dat of the *FREQUENCY step of CalculiX (argument 1)."""
import sys

inside = False
for line in open(sys.argv[1]):
    if "E I G E N V A L U E   O U T P U T" in line:
        inside = True
        continue
    if "P A R T I C I P A T I O N" in line:
        break
    words = line.split()
    if inside and len(words) == 5 and words[0].isdigit():
        print(f"mode {words[0]} frequency {float(words[3]):.7g}")
