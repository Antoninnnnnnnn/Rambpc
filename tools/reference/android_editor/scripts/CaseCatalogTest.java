import org.rambpc.referenceprobe.CaseCatalog;

public final class CaseCatalogTest {
    private static void check(boolean condition, String message) {
        if (!condition) throw new AssertionError(message);
    }
    public static void main(String[] args) {
        for (int id = 1; id <= 3; id++) {
            CaseCatalog test = CaseCatalog.get(id);
            check(test.id == id, "fixed identity");
            check(test.selectionStart >= 0 && test.selectionStart <= test.selectionEnd
                && test.selectionEnd <= test.seed.length(), "valid UTF-16 selection");
            check(test.seed.length() <= CaseCatalog.MAX_TEXT_UTF16, "bounded seed");
        }
        CaseCatalog repeat = CaseCatalog.get(1);
        check(repeat.seed.equals("The draft is ready. The draft is ready for review."), "fixed repeat seed");
        check(repeat.selectionStart == repeat.seed.length() && repeat.selectionEnd == repeat.seed.length(), "repeat cursor at end");
        CaseCatalog rewrite = CaseCatalog.get(2);
        check(rewrite.seed.equals("Send me the report now."), "fixed rewrite seed");
        check(rewrite.selectionStart == 0 && rewrite.selectionEnd == 23, "whole rewrite selected");
        check(CaseCatalog.get(3).seed.isEmpty(), "SEND starts empty");
        check(CaseCatalog.get(3).instruction.equals(
            "Controlled phrase: Send. Empty editor; IME_ACTION_SEND is consumed locally."),
            "SEND has only the fixed Send instruction");
        int[] invalid = {-1, 0, 4, Integer.MIN_VALUE, Integer.MAX_VALUE};
        for (int id : invalid) {
            check(!CaseCatalog.isValid(id), "invalid ID rejected");
            try { CaseCatalog.get(id); throw new AssertionError("invalid ID accepted"); }
            catch (IllegalArgumentException expected) { }
        }
        check("A😀B".length() == 4, "offsets use UTF-16, not code points");
        System.out.println("CaseCatalog host tests passed; no APK/device code executed.");
    }
}
