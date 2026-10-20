// Read-only probe of the 1ba8 orphan switch region.
// @category AI

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.mem.MemoryBlock;

public class Probe1ba8 extends GhidraScript {
    @Override
    protected void run() throws Exception {
        MemoryBlock b = currentProgram.getMemory().getBlock(toAddr("1ba8:0000"));
        if (b == null) b = currentProgram.getMemory().getBlock(toAddr("1ba8:3030"));
        println("BLOCK " + (b == null ? "null" : b.getName() + " " + b.getStart() + "-" + b.getEnd()));
        for (int off : new int[] { 0x2e00, 0x2e27, 0x302f, 0x3030, 0x3032, 0x3100, 0x3200, 0x324e, 0x3253, 0x3709, 0x39ff }) {
            Address a = toAddr("1ba8:" + String.format("%04x", off));
            StringBuilder sb = new StringBuilder();
            for (int i = 0; i < 12; i++) {
                try { sb.append(String.format("%02x ", currentProgram.getMemory().getByte(a.add(i)) & 0xff)); }
                catch (Exception e) { break; }
            }
            Function f = currentProgram.getFunctionManager().getFunctionContaining(a);
            println(String.format("ADDR %s bytes=%s func=%s", a, sb.toString().trim(),
                    f == null ? "null" : f.getName() + "@" + f.getEntryPoint()));
        }
        println("PROLOGUES in 1ba8 2e00-3a10:");
        for (int off = 0x2e00; off < 0x3a10; off++) {
            Address a = toAddr("1ba8:" + String.format("%04x", off));
            try {
                if ((currentProgram.getMemory().getByte(a) & 0xff) == 0x55
                        && (currentProgram.getMemory().getByte(a.add(1)) & 0xff) == 0x8b
                        && (currentProgram.getMemory().getByte(a.add(2)) & 0xff) == 0xec) {
                    println("PRO " + a);
                }
            }
            catch (Exception e) { break; }
        }
    }
}
