# Run from the repo root: vhs demo/demo.tape

Output demo/demo.gif

Set Shell "bash"
Set FontSize 16
Set Width 1100
Set Height 680
Set Padding 20
Set TypingSpeed 40ms

Hide
Type `cargo build --release --quiet && export PATH="$PWD/target/release:$PATH" PS1='$ ' && clear`
Enter
Wait@10m /^\$ *$/
Show

Type "cat demo/sample/cart.py"
Sleep 500ms
Enter
Sleep 3s

Ctrl+L
Type "pithpack demo/sample"
Sleep 500ms
Enter
Sleep 6s

Ctrl+L
Type "pithpack -2 demo/sample"
Sleep 500ms
Enter
Sleep 6s
