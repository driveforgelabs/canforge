// Lists all references to a data address, with reference type and containing function.
// Args: <outfile> <hexAddr>
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import java.io.*;

public class FindDataXrefs extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] a = getScriptArgs();
        Address target = toAddr(a[1]);
        FunctionManager fm = currentProgram.getFunctionManager();
        ReferenceManager rm = currentProgram.getReferenceManager();
        try (PrintWriter w = new PrintWriter(new BufferedWriter(new FileWriter(a[0])))) {
            for (Reference r : rm.getReferencesTo(target)) {
                Address from = r.getFromAddress();
                Function cf = fm.getFunctionContaining(from);
                w.println(from + "  " + r.getReferenceType()
                        + (r.getReferenceType().isWrite() ? " [WRITE]" : "")
                        + "  " + (cf != null ? cf.getName() + " @ " + cf.getEntryPoint() : "[no func]"));
            }
        }
    }
}
