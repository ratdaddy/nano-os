import gdb
import sys

from gdb_providers import children_of_btree_map, unwrap_unique_or_non_null  # pyright: ignore[reportMissingImports]


def _get_blockdevs():
    """Look up BLOCKDEVS and return the inner BTreeMap, or None."""
    sym = gdb.lookup_static_symbol("kernel::dev::block::BLOCKDEVS")
    if sym is None:
        print("Symbol not found — is the kernel ELF loaded?")
        return None
    try:
        inner = sym.value()['inner']['data']['value']
    except gdb.error as e:
        print(f"Failed to read BLOCKDEVS: {e}")
        return None
    return inner


class InfoBlockDevs(gdb.Command):
    """info blockdevs -- list all registered block devices"""

    def __init__(self):
        super().__init__("info blockdevs", gdb.COMMAND_STATUS)

    def invoke(self, argument, from_tty):
        _ = argument
        _ = from_tty

        btree = _get_blockdevs()
        if btree is None:
            return

        entries = list(children_of_btree_map(btree))
        if not entries:
            print("BLOCKDEVS: no devices registered")
            return

        n = len(entries)
        print(f"BLOCKDEVS ({n} device{'s' if n != 1 else ''}):")
        for key, val in entries:
            major = int(key['__0'])
            minor = int(key['__1'])
            print(f"  ({major}, {minor})  {val['name']}")


class InfoBlockDev(gdb.Command):
    """info blockdev <major> <minor> -- print name and LRU cache for one block device"""

    def __init__(self):
        super().__init__("info blockdev", gdb.COMMAND_STATUS)

    @staticmethod
    def _arc_dyn_to_cached_volume(volume):
        arc_inner = unwrap_unique_or_non_null(volume['ptr']).dereference()
        return arc_inner['data'].cast(gdb.lookup_type('kernel::block::cache::CachedVolume'))

    @staticmethod
    def _print_lru(cached_volume):
        slots = cached_volume['cache']['inner']['data']['value']['slots']
        buf_inner = slots['buf']['inner']

        head = int(slots['head'])
        length = int(slots['len'])

        cap = buf_inner['cap']
        if cap.type.code != gdb.TYPE_CODE_INT:
            cap = cap['__0']
        cap = int(cap)

        data_ptr = unwrap_unique_or_non_null(buf_inner['ptr'])
        elem_type = gdb.Type.pointer(slots.type.template_argument(0))
        data_ptr = data_ptr.reinterpret_cast(elem_type)

        print(f"  LRU cache: head={head} len={length} cap={cap}")

        rows = []
        for i in range(length):
            idx = (head + i) % cap
            elem = data_ptr[idx]
            block_id = int(elem['__0'])
            arc = elem['__1']
            strong = int(unwrap_unique_or_non_null(arc['ptr']).dereference()['strong']['v']['value'])
            pinned = " PINNED" if strong > 1 else ""
            rows.append((i, block_id, strong, pinned))

        slot_w  = len(str(length - 1))
        block_w = max(len(str(r[1])) for r in rows) if rows else 1
        for slot, block_id, strong, pinned in rows:
            print(f"    slot {slot:{slot_w}}: block={block_id:{block_w}}  arc_strong={strong}{pinned}")

    def invoke(self, argument, from_tty):
        _ = from_tty

        args = argument.strip().split()
        if len(args) != 2:
            print("Usage: info blockdev <major> <minor>")
            return

        major, minor = int(args[0]), int(args[1])

        btree = _get_blockdevs()
        if btree is None:
            return

        for key, val in children_of_btree_map(btree):
            if int(key['__0']) == major and int(key['__1']) == minor:
                print(f"blockdev ({major}, {minor})  {val['name']}")
                self._print_lru(self._arc_dyn_to_cached_volume(val['volume']))
                return

        print(f"No device found for ({major}, {minor})")


class PrintFields(gdb.Command):
    """print-fields <varname> -- print fields of a module-level gdb value variable"""

    def __init__(self):
        super().__init__("print-fields", gdb.COMMAND_USER)

    def invoke(self, argument, from_tty):
        _ = from_tty
        varname = argument.strip()
        module = sys.modules[__name__]
        val = getattr(module, varname, None)
        if val is None:
            print(f"No variable '{varname}' found")
            return
        for f in val.type.fields():
            print(repr(f.name), f.type)


InfoBlockDevs()
InfoBlockDev()
PrintFields()
