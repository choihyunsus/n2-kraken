# Inspect the actual release archive, executable modes and Windows runtime dependencies.
import json
import struct
import sys
import tarfile


def pe_imports(data):
    pe = struct.unpack_from('<I', data, 60)[0]
    assert data[:2] == b'MZ' and data[pe:pe + 4] == b'PE\0\0'
    count = struct.unpack_from('<H', data, pe + 6)[0]
    optional = pe + 24
    size = struct.unpack_from('<H', data, pe + 20)[0]
    magic = struct.unpack_from('<H', data, optional)[0]
    directory = optional + (112 if magic == 0x20B else 96)
    address, length = struct.unpack_from('<II', data, directory + 8)
    sections = [struct.unpack_from('<IIII', data, optional + size + index * 40 + 8)
                for index in range(count)]

    def file_offset(rva):
        for virtual_size, virtual_address, raw_size, raw_pointer in sections:
            if virtual_address <= rva < virtual_address + max(virtual_size, raw_size):
                return raw_pointer + rva - virtual_address
        raise ValueError('Unmapped PE address')

    imported = []
    start = file_offset(address)
    for index in range(length // 20):
        fields = struct.unpack_from('<IIIII', data, start + index * 20)
        if not any(fields):
            break
        name = file_offset(fields[3])
        imported.append(data[name:data.index(b'\0', name)].decode('ascii'))
    return imported


with tarfile.open(sys.argv[1]) as archive:
    entries = archive.getmembers()
    workers = [entry for entry in entries if entry.name.startswith('package/bin/n2-kraken-worker-')]
    assert len(workers) == 3, [entry.name for entry in workers]
    assert all(entry.isfile() and entry.mode & 0o111 == 0o111 for entry in workers)
    assert not any('/target/' in entry.name or '/node_modules/' in entry.name for entry in entries)
    assert not any(entry.issym() or entry.islnk() or '..' in entry.name.split('/') for entry in entries)
    windows = next(entry for entry in workers if entry.name.endswith('.exe'))
    imports = pe_imports(archive.extractfile(windows).read())
    assert not any(name.upper().startswith(('MSVCP', 'VCRUNTIME')) for name in imports), imports
    print(json.dumps({'files': len(entries), 'workers': [entry.name for entry in workers], 'windows_imports': imports}))
