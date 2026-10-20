// Define driver stub/body functions for imported raw DVR binaries.
// MCGA.DVR: 10 x 3-byte E9 near jumps at 0x7a.
// ADLIB.DVR / IBM.DVR: 13 x 10-byte CALL/CALL/CALL/RETF stubs at 0x32.
// @category AI

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.mem.Memory;

public class DefineDriverStubs extends GhidraScript {

    @Override
    protected void run() throws Exception {
        String name = currentProgram.getName().toUpperCase();
        Memory mem = currentProgram.getMemory();
        if (name.contains("MCGA")) {
            defineMcga(mem);
        }
        else if (name.contains("ADLIB") || name.contains("IBM")) {
            defineSound(mem);
        }
        else {
            println("Unknown driver: " + name);
        }
    }

    private void defineMcga(Memory mem) throws Exception {
        int off = 0x7a;
        int n = 0;
        while (n < 32) {
            Address a = toAddr(off + n * 3);
            try {
                if ((mem.getByte(a) & 0xff) != 0xe9) {
                    break;
                }
                int rel = mem.getShort(a.add(1));
                Address body = a.add(3 + rel);
                makeStub(a, "drv_" + n + "_stub", "MCGA entry " + n);
                makeBody(body, "drv_" + n);
                println(String.format("MCGA %d stub=%s body=%s", n, a, body));
            }
            catch (Exception e) {
                break;
            }
            n++;
        }
        println("MCGA_STUBS=" + n);
    }

    private void defineSound(Memory mem) throws Exception {
        int off = 0x32;
        int n = 0;
        while (n < 32) {
            Address a = toAddr(off + n * 10);
            try {
                if ((mem.getByte(a) & 0xff) != 0xe8
                        || (mem.getByte(a.add(3)) & 0xff) != 0xe8
                        || (mem.getByte(a.add(6)) & 0xff) != 0xe8
                        || (mem.getByte(a.add(9)) & 0xff) != 0xcb) {
                    break;
                }
                int r0 = mem.getShort(a.add(1));
                int r1 = mem.getShort(a.add(4));
                int r2 = mem.getShort(a.add(7));
                Address prolog = a.add(3 + r0);
                Address body = a.add(6 + r1);
                Address epilog = a.add(9 + r2);
                makeStub(a, "drv_" + n + "_stub", "sound entry " + n);
                makeBody(prolog, "drv_" + n + "_prolog");
                makeBody(body, "drv_" + n);
                makeBody(epilog, "drv_" + n + "_epilog");
                println(String.format("%s %d stub=%s prolog=%s body=%s epilog=%s",
                        currentProgram.getName(), n, a, prolog, body, epilog));
            }
            catch (Exception e) {
                break;
            }
            n++;
        }
        println("SOUND_STUBS=" + n);
    }

    private void makeStub(Address a, String name, String comment) {
        try {
            if (currentProgram.getListing().getInstructionAt(a) == null) {
                disassemble(a);
            }
            Function f = createFunction(a, name);
            if (f != null) {
                f.setComment(comment);
            }
        }
        catch (Exception e) {
            println("stub failed " + a + ": " + e.getMessage());
        }
    }

    private void makeBody(Address a, String name) {
        try {
            if (currentProgram.getListing().getInstructionAt(a) == null) {
                disassemble(a);
            }
            createFunction(a, name);
        }
        catch (Exception e) {
            println("body failed " + a + ": " + e.getMessage());
        }
    }
}
