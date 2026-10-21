package app.linkall

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class VerificationCodeTest {
    @Test fun githubStyle() = assertEquals("482910", VerificationCode.find("Your GitHub launch code is 482910"))

    @Test fun bankStyleWithAnAmount() =
        assertEquals("739201", VerificationCode.find("OTP 739201 for txn of Rs 1,500.00 at AMAZON. Do not share it with anyone."))

    @Test fun googleStyle() = assertEquals("123456", VerificationCode.find("G-123456 is your Google verification code."))

    @Test fun codeWithASeparator() = assertEquals("482910", VerificationCode.find("Your verification code: 482-910"))

    @Test fun numberBeforeTheKeyword() = assertEquals("5521", VerificationCode.find("5521 is your one-time password"))

    @Test fun anAmountIsNotACode() = assertNull(VerificationCode.find("Your balance is Rs 12345. Thank you for banking with us."))

    @Test fun aMessageWithoutAKeywordIsIgnored() = assertNull(VerificationCode.find("Call me on 0123456789 when you land"))

    @Test fun aPercentageIsNotACode() = assertNull(VerificationCode.find("Use code SAVE and get 2025% back"))

    @Test fun tooShortOrTooLong() {
        assertNull(VerificationCode.find("Your code is 123"))
        assertNull(VerificationCode.find("Your code is 123456789012"))
    }

    @Test fun emptyText() = assertNull(VerificationCode.find(""))
}
