package app.linkall

/**
 * Finds a one-time verification code in a message, such as the 6 digits a
 * bank or GitHub texts you. Pure text work, so it can be tested without a phone.
 *
 * A message counts only when it says what the number is for ("code",
 * "OTP", "verify" ...), and the code is the 4 to 8 digit number nearest
 * after such a word, so an amount, a year or a phone number in the same
 * message is not picked.
 */
object VerificationCode {
    private val KEYWORDS = Regex(
        "code|otp|one[- ]time|verif|passcode|password|\\bpin\\b|authenticat|security|sign[- ]?in|log[- ]?in",
        RegexOption.IGNORE_CASE
    )

    // 4 to 8 digits, or two groups of three or four ("482 910", "482-910").
    private val NUMBER = Regex("(?<![\\d.,])(?:G-)?(\\d{3,4}[- ]\\d{3,4}|\\d{4,8})(?![\\d])")

    /** How far after a keyword the code may sit. */
    private const val REACH = 48

    /** Amounts and percentages are not codes. */
    private val NOT_A_CODE_BEFORE = Regex("(?:rs\\.?|inr|usd|eur|gbp|\\$|€|£|₹|amount|balance|bal)\\s*$", RegexOption.IGNORE_CASE)
    private val NOT_A_CODE_AFTER = Regex("^\\s*(?:%|rs|inr|usd|eur|gbp|/-|\\.\\d)", RegexOption.IGNORE_CASE)

    /** The code in [text], digits only, or null when it holds none. */
    fun find(text: String): String? {
        if (text.length < 6 || text.length > 600) return null
        val keywords = KEYWORDS.findAll(text).map { it.range }.toList()
        if (keywords.isEmpty()) return null
        val candidates = NUMBER.findAll(text).filter { m ->
            val before = text.substring(0, m.range.first)
            val after = text.substring(m.range.last + 1)
            !NOT_A_CODE_BEFORE.containsMatchIn(before.takeLast(14)) &&
                !NOT_A_CODE_AFTER.containsMatchIn(after.take(8))
        }.toList()
        if (candidates.isEmpty()) return null
        // The first number that follows a keyword closely, else one right before it.
        val best = candidates.firstOrNull { m ->
            keywords.any { k -> m.range.first > k.last && m.range.first - k.last <= REACH }
        } ?: candidates.firstOrNull { m ->
            keywords.any { k -> k.first > m.range.last && k.first - m.range.last <= REACH }
        } ?: return null
        val digits = best.groupValues[1].filter { it.isDigit() }
        return digits.takeIf { it.length in 4..8 }
    }
}
