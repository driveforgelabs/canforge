// Lists references to a target address. For each data reference, dumps the surrounding pointer
// table (the vtable) as address:function so we can read the whole class dispatch.
// Args: <outfile> <hexAddr>
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import ghidra.program.model.mem.*;
import java.io.*;

public class FindVtable extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] a = getScriptArgs();
        Address target = toAddr(a[1]);
        Memory mem = currentProgram.getMemory();
        FunctionManager fm = currentProgram.getFunctionManager();
        try (PrintWriter w = new PrintWriter(new BufferedWriter(new FileWriter(a[0])))) {
            ReferenceManager rm = currentProgram.getReferenceManager();
            for (Reference r : rm.getReferencesTo(target)) {
                Address from = r.getFromAddress();
                Function cf = fm.getFunctionContaining(from);
                w.println("REF from " + from + " type=" + r.getReferenceType()
                        + (cf != null ? " in " + cf.getName() : " [data]"));
                if (cf == null) {
                    // likely a vtable slot: dump pointers around it
                    long base = from.getOffset() & ~3L;
                    w.println("  -- vtable window around " + from + " --");
                    for (long off = -0x20; off <= 0x40; off += 4) {
                        try {
                            Address slot = target.getNewAddress(base + off);
                            long p = mem.getInt(slot) & 0xffffffffL;
                            Function pf = fm.getFunctionAt(target.getNewAddress(p));
                            String nm = pf != null ? pf.getName() : "";
                            w.printf("  [+0x%x] %s -> %08x %s%n", off, slot, p, nm);
                        } catch (Exception e) { /* skip */ }
                    }
                }
            }
        }
    }
}
