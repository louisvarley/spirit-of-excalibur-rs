// Rename already-created driver functions after auto-analysis.
// @category AI

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.symbol.SourceType;

public class RenameDriverStubs extends GhidraScript {
    @Override
    protected void run() throws Exception {
        String name = currentProgram.getName().toUpperCase();
        Memory mem = currentProgram.getMemory();
        if (name.contains("MCGA")) {
            for (int n = 0; n < 10; n++) {
                Address a = toAddr(0x7a + n * 3);
                int rel = mem.getShort(a.add(1));
                rename(a, "drv_" + n + "_stub");
                rename(a.add(3 + rel), "drv_" + n);
            }
        }
        else if (name.contains("ADLIB") || name.contains("IBM")) {
            for (int n = 0; n < 13; n++) {
                Address a = toAddr(0x32 + n * 10);
                int r0 = mem.getShort(a.add(1));
                int r1 = mem.getShort(a.add(4));
                int r2 = mem.getShort(a.add(7));
                rename(a, "drv_" + n + "_stub");
                rename(a.add(3 + r0), "drv_" + n + "_prolog");
                rename(a.add(6 + r1), "drv_" + n);
                rename(a.add(9 + r2), "drv_" + n + "_epilog");
            }
        }
    }

    private void rename(Address a, String name) {
        Function f = currentProgram.getFunctionManager().getFunctionAt(a);
        if (f == null) {
            try {
                f = createFunction(a, name);
            }
            catch (Exception e) {
                println("create failed " + a + ": " + e.getMessage());
            }
        }
        if (f != null) {
            try {
                f.setName(name, SourceType.USER_DEFINED);
            }
            catch (Exception e) {
                println("rename failed " + a + ": " + e.getMessage());
            }
        }
    }
}
