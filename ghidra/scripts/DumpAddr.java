// Read-only address mapping probe.
// @category AI

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.mem.MemoryBlock;

public class DumpAddr extends GhidraScript {
    @Override
    protected void run() throws Exception {
        probe("2d65:0adf");
        probe("2d65:0ae3");
        probe("1000:0102");
        probe("1316:0d87");
    }

    private void probe(String a) {
        Address addr = toAddr(a);
        Instruction ins = currentProgram.getListing().getInstructionAt(addr);
        if (ins == null) {
            println("NO INSN " + a);
            return;
        }
        MemoryBlock b = currentProgram.getMemory().getBlock(addr);
        println("INSN " + ins.getAddress() + " off=0x" + Long.toHexString(ins.getAddress().getOffset())
                + " block=" + (b == null ? "null" : b.getName() + " start=" + b.getStart()
                + " startOff=0x" + Long.toHexString(b.getStart().getOffset()))
                + " text=" + ins);
        for (ghidra.program.model.symbol.Reference r : ins.getReferencesFrom()) {
            Address t = r.getToAddress();
            Function f = currentProgram.getFunctionManager().getFunctionAt(t);
            if (f == null) {
                f = currentProgram.getFunctionManager().getFunctionContaining(t);
            }
            println("  REF " + r.getReferenceType() + " " + t + " off=0x"
                    + Long.toHexString(t.getOffset()) + " func="
                    + (f == null ? "null" : f.getName() + "@" + f.getEntryPoint()
                    + " entryOff=0x" + Long.toHexString(f.getEntryPoint().getOffset())));
        }
    }
}
