// Decompiles every function whose entry lies in [start,end] (hex addresses).
// Args: <outfile> <startHex> <endHex>
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import java.io.*;

public class DumpRange extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] a = getScriptArgs();
        Address lo = toAddr(a[1]), hi = toAddr(a[2]);
        DecompInterface ifc = new DecompInterface();
        ifc.openProgram(currentProgram);
        try (PrintWriter w = new PrintWriter(new BufferedWriter(new FileWriter(a[0])))) {
            FunctionIterator it = currentProgram.getFunctionManager().getFunctions(lo, true);
            while (it.hasNext() && !monitor.isCancelled()) {
                Function f = it.next();
                if (f.getEntryPoint().compareTo(hi) > 0) break;
                DecompileResults r = ifc.decompileFunction(f, 90, monitor);
                w.println("\n// ---- " + f.getName() + " @ " + f.getEntryPoint());
                w.println(r.decompileCompleted() ? r.getDecompiledFunction().getC() : "// failed");
            }
        }
    }
}
