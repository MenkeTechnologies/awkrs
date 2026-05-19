# gawk parity: in paragraph mode (RS == ""), RT contains the FULL run of
# newlines/blank lines between records — not just a single "\n". Earlier
# awkrs reported only "\n" regardless of how many blank lines separated the
# records.
BEGIN { RS="" } { print NR, "[" RT "]" }
