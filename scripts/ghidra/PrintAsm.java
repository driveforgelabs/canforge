// Prints disassembly for an address range, with any constant moved into ECX/arg before each call.
// Args: <outfile> <startHex> <endHex>
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import java.io.*;

public class PrintAsm extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] a = getScriptArgs();
        Address lo = toAddr(a[1]), hi = toAddr(a[2]);
        Listing L = currentProgram.getListing();
        try (PrintWriter w = new PrintWriter(new BufferedWriter(new FileWriter(a[0])))) {
            InstructionIterator it = L.getInstructions(lo, true);
            while (it.hasNext()) {
                Instruction in = it.next();
                if (in.getAddress().compareTo(hi) > 0) break;
                w.println(in.getAddress() + "  " + in);
            }
        }
    }
}
