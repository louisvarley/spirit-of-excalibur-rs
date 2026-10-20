// Read-only relocation table probe.
// @category AI

import java.util.Iterator;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.reloc.Relocation;
import ghidra.program.model.reloc.RelocationTable;

public class DumpRelocs extends GhidraScript {
    @Override
    protected void run() throws Exception {
        RelocationTable rt = currentProgram.getRelocationTable();
        println("RELOCS size=" + rt.getSize());
        Iterator<Relocation> it = rt.getRelocations();
        int n = 0;
        while (it.hasNext()) {
            Relocation r = it.next();
            n++;
            if (n <= 25) {
                long[] vals = r.getValues();
                StringBuilder vs = new StringBuilder();
                if (vals != null) {
                    for (long v : vals) {
                        vs.append(String.format("0x%x ", v));
                    }
                }
                int mem = -1;
                try {
                    mem = currentProgram.getMemory().getShort(r.getAddress()) & 0xffff;
                }
                catch (Exception e) {
                    // ignore
                }
                println("REL " + r.getAddress() + " type=" + r.getType() + " status="
                        + r.getStatus() + " values=[" + vs.toString().trim() + "] bytes="
                        + r.getBytes().length + " mem=0x" + Integer.toHexString(mem)
                        + " sym=" + r.getSymbolName());
            }
        }
        println("RELOCS read=" + n);
    }
}
