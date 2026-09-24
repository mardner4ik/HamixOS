#!/usr/bin/env python3
import struct
import sys


def main():
    if len(sys.argv) != 2:
        print("usage: device-ids.py module.o", file=sys.stderr)
        return 2
    data = open(sys.argv[1], "rb").read()
    if data[:4] != b"\x7fELF" or data[4] != 2:
        print("not a 64-bit ELF object", file=sys.stderr)
        return 1
    shoff, = struct.unpack_from("<Q", data, 0x28)
    shentsize, shnum, shstrndx = struct.unpack_from("<HHH", data, 0x3A)
    sections = []
    for i in range(shnum):
        off = shoff + i * shentsize
        name, kind, flags, addr, offset, size, link, info, align, entsize = struct.unpack_from("<IIQQQQIIQQ", data, off)
        sections.append((name, kind, offset, size, link, entsize))
    lines = []
    for name, kind, offset, size, link, entsize in sections:
        if kind != 2:
            continue
        strtab = sections[link]
        for at in range(offset, offset + size, 24):
            st_name, st_info, st_other, st_shndx, st_value, st_size = struct.unpack_from("<IBBHQQ", data, at)
            end = data.index(b"\0", strtab[2] + st_name)
            symbol = data[strtab[2] + st_name:end].decode()
            if not (symbol.startswith("__mod_pci__") and symbol.endswith("_device_table")):
                continue
            target = sections[st_shndx]
            base = target[2] + st_value
            count = st_size // 40 if st_size else 4096
            for index in range(count):
                entry = base + index * 40
                if entry + 40 > len(data):
                    break
                vendor, device, subvendor, subdevice, klass, mask = struct.unpack_from("<IIIIII", data, entry)
                if vendor == 0 and subvendor == 0 and mask == 0:
                    break
                if vendor == 0xFFFFFFFF:
                    cc = (klass >> 16) & 0xFF
                    ss = (klass >> 8) & 0xFF if mask & 0xFF00 else None
                    pp = klass & 0xFF if mask & 0xFF else None
                    lines.append("class:%02x:%s:%s" % (cc, "*" if ss is None else "%02x" % ss, "*" if pp is None else "%02x" % pp))
                elif device == 0xFFFFFFFF:
                    continue
                else:
                    lines.append("%04x:%04x" % (vendor, device))
    seen = set()
    print("# Generated from MODULE_DEVICE_TABLE by tools/linuxkpi/device-ids.py")
    for line in lines:
        if line not in seen:
            seen.add(line)
            print(line)
    return 0 if lines else 1


if __name__ == "__main__":
    sys.exit(main())
