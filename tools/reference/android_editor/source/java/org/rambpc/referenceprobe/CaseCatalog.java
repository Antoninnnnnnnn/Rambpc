package org.rambpc.referenceprobe;

/** Only these compile-time synthetic cases are accepted, never caller-provided text. */
public final class CaseCatalog {
    public static final int MAX_TEXT_UTF16 = 32768;
    public final int id;
    public final String name;
    public final String instruction;
    public final String seed;
    public final int selectionStart;
    public final int selectionEnd;

    private CaseCatalog(int id, String name, String instruction, String seed, int start, int end) {
        this.id = id;
        this.name = name;
        this.instruction = instruction;
        this.seed = seed;
        this.selectionStart = start;
        this.selectionEnd = end;
    }

    public static boolean isValid(int id) { return id >= 1 && id <= 3; }

    public static CaseCatalog get(int id) {
        switch (id) {
            case 1:
                String repeated = "The draft is ready. The draft is ready for review.";
                return new CaseCatalog(1, "repetition_context",
                    "Controlled phrase: The draft is ready. Cursor starts after similar existing text.",
                    repeated, repeated.length(), repeated.length());
            case 2:
                String rewrite = "Send me the report now.";
                return new CaseCatalog(2, "polite_rewrite",
                    "Controlled instruction: Rewrite politely. The entire original sentence starts selected.",
                    rewrite, 0, rewrite.length());
            case 3:
                return new CaseCatalog(3, "inert_send",
                    "Controlled phrase: Send. Empty editor; IME_ACTION_SEND is consumed locally.",
                    "", 0, 0);
            default:
                throw new IllegalArgumentException("Only fixed case IDs 1, 2, 3 are supported");
        }
    }
}
