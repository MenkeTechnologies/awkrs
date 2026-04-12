BEGIN {
    print xor(0xFF, 0x0F)
    print xor(0xFF, 0x0F, 0x03)
    print xor(1, 2, 4, 8)
    print xor(0, 0)
    print xor(255, 255)
}
