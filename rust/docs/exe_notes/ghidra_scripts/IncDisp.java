import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.*;
import ghidra.program.model.scalar.*;
import java.io.*;
import java.util.*;

// args: outfile ; per function, displacements used by INC / ADD-immediate-1 instructions (version counters)
public class IncDisp extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] a = getScriptArgs();
        PrintWriter pw = new PrintWriter(new FileWriter(a[0], false));
        Map<Long, TreeSet<Long>> hits = new TreeMap<>();
        InstructionIterator it = currentProgram.getListing().getInstructions(true);
        while (it.hasNext() && !monitor.isCancelled()) {
            Instruction in = it.next();
            String m = in.getMnemonicString().toLowerCase();
            if (!(m.equals("inc") || m.equals("add"))) continue;
            if (in.getNumOperands() < 1) continue;
            String op0 = in.getDefaultOperandRepresentation(0);
            if (!op0.contains("[")) continue;
            if (m.equals("add")) {
                if (in.getNumOperands() < 2) continue;
                Object[] o1 = in.getOpObjects(1);
                if (o1.length != 1 || !(o1[0] instanceof Scalar) || ((Scalar) o1[0]).getSignedValue() != 1) continue;
            }
            for (Object o : in.getOpObjects(0)) {
                if (o instanceof Scalar) {
                    long v = ((Scalar) o).getSignedValue();
                    if (v > 0 && v < 0x100000) {
                        Function f = getFunctionContaining(in.getAddress());
                        if (f == null) continue;
                        hits.computeIfAbsent(f.getEntryPoint().getOffset(), k -> new TreeSet<>()).add(v);
                    }
                }
            }
        }
        for (Map.Entry<Long, TreeSet<Long>> e : hits.entrySet()) {
            StringBuilder sb = new StringBuilder(Long.toHexString(e.getKey()));
            for (long v : e.getValue()) sb.append(' ').append(Long.toHexString(v));
            pw.println(sb);
        }
        pw.close();
    }
}
