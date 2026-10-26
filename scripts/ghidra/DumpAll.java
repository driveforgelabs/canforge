// Dumps exports, defined strings and decompiled functions to a text file.
// Usage (headless): -postScript DumpAll.java <outfile>
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import ghidra.program.model.data.*;
import java.io.*;

public class DumpAll extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        File out = new File(args[0]);
        try (PrintWriter w = new PrintWriter(new BufferedWriter(new FileWriter(out)))) {
            w.println("## PROGRAM " + currentProgram.getName() + " base=" + currentProgram.getImageBase());

            w.println("\n## EXPORTS");
            SymbolIterator si = currentProgram.getSymbolTable().getAllSymbols(true);
            while (si.hasNext()) {
                Symbol s = si.next();
                if (s.isExternalEntryPoint())
                    w.println(s.getAddress() + " " + s.getName());
            }

            w.println("\n## IMPORTS");
            for (Symbol s : currentProgram.getSymbolTable().getExternalSymbols())
                w.println(s.getParentNamespace().getName() + "!" + s.getName());

            w.println("\n## STRINGS");
            DataIterator di = currentProgram.getListing().getDefinedData(true);
            while (di.hasNext()) {
                Data d = di.next();
                if (d.hasStringValue())
                    w.println(d.getAddress() + " " + d.getDefaultValueRepresentation());
            }

            w.println("\n## FUNCTIONS");
            DecompInterface ifc = new DecompInterface();
            ifc.openProgram(currentProgram);
            FunctionIterator fi = currentProgram.getFunctionManager().getFunctions(true);
            while (fi.hasNext() && !monitor.isCancelled()) {
                Function f = fi.next();
                DecompileResults r = ifc.decompileFunction(f, 60, monitor);
                w.println("\n// ---- " + f.getName() + " @ " + f.getEntryPoint());
                if (r.decompileCompleted())
                    w.println(r.getDecompiledFunction().getC());
                else
                    w.println("// decompile failed: " + r.getErrorMessage());
            }
        }
    }
}
