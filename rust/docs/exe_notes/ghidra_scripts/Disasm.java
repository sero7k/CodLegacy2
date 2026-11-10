import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.*;
import ghidra.program.model.address.*;
import java.io.*;

// args: outfile start end ; prints instructions (disassembling if needed) and refs-to for the start of each
public class Disasm extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] a = getScriptArgs();
        PrintWriter pw = new PrintWriter(new FileWriter(a[0], false));
        Address s = toAddr(Long.decode(a[1])), e = toAddr(Long.decode(a[2]));
        disassemble(s);
        Address cur = s;
        while (cur.compareTo(e) < 0) {
            Instruction in = getInstructionAt(cur);
            if (in == null) { disassemble(cur); in = getInstructionAt(cur); }
            if (in == null) { pw.println(cur + " ??"); cur = cur.add(1); continue; }
            StringBuilder refs = new StringBuilder();
            for (ghidra.program.model.symbol.Reference r : getReferencesTo(cur)) refs.append(" <-").append(r.getFromAddress());
            pw.println(cur + "  " + in + refs);
            cur = cur.add(in.getLength());
        }
        pw.close();
    }
}
