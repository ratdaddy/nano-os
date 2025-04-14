# Current debug process:
1. Run qemu:
```bash
make qemu-debug
```

2. Run the ubuntu docker image
```bash
make gdb
```

3. Set breakpoints and debug:
```bash
b *0x80200000
x/20i $pc
x/20gx 0x80201000 # displays 20 locations of page table
```

## To make a new docker image:
```bash
make gdb-docker
```
