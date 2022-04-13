import PayInvoiceForm from "../components/PayInvoiceForm";
import PaymentsList from "../components/PaymentsList";
const SendMoneyPage = () => {
  return (
    <div className="py-6">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
        <h1 className="text-2xl font-semibold text-light-plum">Send Money</h1>
      </div>
      <div className="max-w-7xl mx-auto px-4 sm:px-6 md:px-8">
        <div className="py-4">
          <div className="bg-plum-100 shadow p-4 rounded-xl">
            <PayInvoiceForm />
          </div>
        </div>
      </div>

      <div className="mt-8 max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
        <h1 className="text-2xl font-semibold text-light-plum">
          Outgoing Payments
        </h1>
      </div>
      <div className="max-w-7xl mx-auto px-4 sm:px-6 md:px-8">
        <div className="py-4">
          <PaymentsList origin="outgoing" />
        </div>
      </div>
    </div>
  );
};

export default SendMoneyPage;
