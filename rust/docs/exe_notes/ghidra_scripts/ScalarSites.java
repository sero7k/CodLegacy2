import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.*;
import ghidra.program.model.scalar.Scalar;
import java.util.*;
import java.io.*;
// args: out_file v1 v2 ... ; prints every instruction with one of these scalar operands
public class ScalarSites extends GhidraScript {
  public void run() throws Exception {
    String[] a = getScriptArgs();
    Set<Long> want = new HashSet<>();
    for (int i = 1; i < a.length; i++) want.add(Long.decode(a[i]));
    StringBuilder sb = new StringBuilder();
    InstructionIterator ii = currentProgram.getListing().getInstructions(true);
    while (ii.hasNext()) {
      Instruction ins = ii.next();
      for (int o = 0; o < ins.getNumOperands(); o++) {
        for (Object x : ins.getOpObjects(o)) if (x instanceof Scalar) {
          long v = ((Scalar)x).getUnsignedValue() & 0xffffffffL;
          if (want.contains(v)) {
            Function f = getFunctionContaining(ins.getAddress());
            sb.append(String.format("%08x %s %s | %s\n", v, ins.getAddress(), f==null?"?":f.getEntryPoint().toString(), ins.toString()));
          }
        }
      }
    }
    try (FileWriter w = new FileWriter(a[0])) { w.write(sb.toString()); }
    println("wrote "+a[0]);
  }
}
